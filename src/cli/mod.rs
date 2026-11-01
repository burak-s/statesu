//! The `statesu-cli` terminal client: create, edit, publish and delete posts
//! on a running Statesu server over its HTTP API. There is deliberately no
//! Git and no local post storage: the server is the source of truth, and
//! locally only connection settings and the saved token persist.

mod client;
mod editor;
mod error;
mod output;
mod settings;

use std::env;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

pub use error::CliError;

use crate::ids::valid_slug;
use crate::post::service::{MAX_CONTENT_BYTES, MAX_TITLE_BYTES};
use crate::post::{STATUS_DRAFT, STATUS_PUBLISHED};
use settings::Settings;

/// Server URL and token overrides for automation environments.
const SERVER_ENV: &str = "STATESU_SERVER";
const TOKEN_ENV: &str = "STATESU_TOKEN";

/// Flags shared by every subcommand.
#[derive(Debug, clap::Args)]
pub struct Shared {
    /// CMS base URL, overriding the stored server and STATESU_SERVER.
    #[arg(long, global = true)]
    pub server: Option<String>,
    /// Blog id or slug for this command, overriding the stored selection.
    #[arg(long, global = true)]
    pub blog: Option<String>,
    /// Print machine-readable JSON on stdout.
    #[arg(long, global = true)]
    pub json: bool,
}

/// Top-level CLI: parsed by the `statesu-cli` binary target.
#[derive(Debug, Parser)]
#[command(
    name = "statesu-cli",
    version,
    about = "Terminal client for the Statesu CMS: write and publish posts without Git."
)]
pub struct Cli {
    #[command(flatten)]
    pub shared: Shared,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Authenticate and save a write-capable API token locally.
    Login,
    /// Check the saved token and show the server and blog summary.
    Whoami,
    /// Revoke the saved token on the server and clear local settings.
    Logout,
    /// Create and select blogs.
    #[command(subcommand)]
    Blog(BlogCommand),
    /// Write, publish and manage posts.
    #[command(subcommand)]
    Post(PostCommand),
}

#[derive(Debug, Subcommand)]
pub enum BlogCommand {
    /// List your blogs.
    List,
    /// Create a blog.
    Create {
        /// Blog display name.
        name: String,
        /// URL slug [default: derived from the name].
        #[arg(long)]
        slug: Option<String>,
    },
    /// Select a blog for later commands in these settings.
    Use {
        /// Slug of the blog to select.
        slug: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum PostCommand {
    /// Write a post in the interactive editor (draft by default).
    New {
        /// Post title.
        title: String,
        /// URL slug [default: derived from the title].
        #[arg(long)]
        slug: Option<String>,
        /// Publish immediately instead of saving a draft.
        #[arg(long)]
        publish: bool,
    },
    /// Create a post from a file or piped stdin; no editor is opened.
    Create {
        /// Post title.
        title: String,
        /// Markdown file to upload [default: piped stdin].
        #[arg(long)]
        file: Option<PathBuf>,
        /// URL slug [default: derived from the title].
        #[arg(long)]
        slug: Option<String>,
        /// Publish immediately instead of saving a draft.
        #[arg(long)]
        publish: bool,
    },
    /// List posts, drafts included.
    List {
        /// 1-based page number [default: 1].
        #[arg(long)]
        page: Option<u32>,
        /// Page size, capped at 100 by the server [default: 20].
        #[arg(long)]
        size: Option<u32>,
    },
    /// Print the raw Markdown of a post.
    Get {
        /// Post id or slug.
        target: String,
    },
    /// Edit a post in the interactive editor; published posts change live.
    Edit {
        /// Post id or slug.
        target: String,
    },
    /// Publish a post.
    Publish {
        /// Post id or slug.
        target: String,
    },
    /// Take a post back into draft.
    Unpublish {
        /// Post id or slug.
        target: String,
    },
    /// Delete a post permanently (confirmation unless --yes).
    Delete {
        /// Post id or slug.
        target: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

impl Cli {
    pub async fn run(self) -> Result<(), CliError> {
        let shared = &self.shared;
        match self.command {
            Command::Login => login(shared.server.as_deref()).await,
            Command::Whoami => whoami(shared).await,
            Command::Logout => logout(shared.server.as_deref()).await,
            Command::Blog(command) => blog_command(shared, command).await,
            Command::Post(command) => post_command(shared, command).await,
        }
    }
}

// Connection context ----------------------------------------------------

/// Per-invocation authenticated context: a validated HTTP API, the token and
/// the settings it came from. Never constructed without a token.
struct Authed {
    api: client::Api,
    server: String,
    pat: String,
    settings: Settings,
    json: bool,
    blog: Option<String>,
}

impl Authed {
    async fn new(shared: &Shared) -> Result<Self, CliError> {
        let settings = Settings::load()?;
        let server = resolve_server(shared.server.as_deref(), &settings)?;
        let api = client::Api::new(&server)?;
        let pat = env::var(TOKEN_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or(settings.pat.clone())
            .ok_or_else(|| {
                CliError::usage("no token saved: run `statesu-cli login` or set STATESU_TOKEN")
            })?;
        Ok(Authed {
            api,
            server,
            pat,
            settings,
            json: shared.json,
            blog: shared.blog.clone(),
        })
    }

    /// Blog target as an id: explicit `--blog`, else the saved selection.
    /// Slugs are resolved through a listing round-trip.
    async fn blog_id(&self) -> Result<String, CliError> {
        let target = self
            .blog
            .as_deref()
            .or(self.settings.blog_id.as_deref())
            .ok_or_else(|| {
                CliError::usage(
                    "no blog selected: run `statesu-cli blog use <slug>` or pass --blog",
                )
            })?;
        if let Ok(id) = parse_uuid(target) {
            return Ok(id);
        }
        let blogs = self.api.blogs(&self.pat).await?;
        blogs
            .iter()
            .find(|blog| blog.slug == target)
            .map(|blog| blog.id.clone())
            .ok_or_else(|| CliError::usage(format!("no blog with slug '{target}'")))
    }

    /// Post target as a full record: ids take a direct GET path, slugs scan
    /// the paginated listing until the total is covered.
    async fn post(&self, blog_id: &str, target: &str) -> Result<client::Post, CliError> {
        if let Ok(id) = parse_uuid(target) {
            return self.api.post(&self.pat, blog_id, &id).await;
        }
        let mut page = 1;
        loop {
            let paged = self
                .api
                .posts(&self.pat, blog_id, page, client::MAX_PAGE)
                .await?;
            if let Some(found) = paged.items.iter().find(|post| post.slug == target) {
                return Ok(found.clone());
            }
            let covered = i64::from(paged.page) * i64::from(paged.size);
            if covered >= paged.total {
                break;
            }
            page += 1;
        }
        Err(CliError::usage(format!("no post '{target}' in this blog")))
    }
}

/// Parses a canonical UUID, trimmed; anything else is meant as a slug.
fn parse_uuid(text: &str) -> Result<String, ()> {
    text.trim()
        .parse::<uuid::Uuid>()
        .map(|uuid| uuid.to_string())
        .map_err(|_| ())
}

/// Server precedence: --server flag, STATESU_SERVER, stored settings.
fn resolve_server(explicit: Option<&str>, settings: &Settings) -> Result<String, CliError> {
    explicit
        .map(str::to_owned)
        .or_else(|| {
            env::var(SERVER_ENV)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| settings.server.clone())
        .ok_or_else(|| {
            CliError::usage(
                "no server set: pass --server or run `statesu-cli login --server <url>`",
            )
        })
}

// Auth commands ---------------------------------------------------------

async fn login(server: Option<&str>) -> Result<(), CliError> {
    let settings = Settings::load()?;
    let server = resolve_server(server, &settings)?;
    let api = client::Api::new(&server)?;
    if !std::io::stdin().is_terminal() {
        return Err(CliError::usage(
            "login needs an interactive terminal; for automation mint a token in the dashboard and set STATESU_TOKEN",
        ));
    }
    let email = prompt("Email: ")?;
    let password = rpassword::prompt_password("Password: ").map_err(CliError::Io)?;
    if email.is_empty() || password.is_empty() {
        return Err(CliError::usage("email and password are required"));
    }
    let (me, session) = api.login(&email, &password).await?;
    let token = api.create_pat(&session, "statesu-cli").await?;
    let mut settings = settings;
    let same_server = settings.server.as_deref() == Some(server.as_str());
    settings.server = Some(server);
    settings.pat = Some(token.secret);
    settings.pat_id = Some(token.id);
    // A token from another server must never carry a selection across.
    if !same_server {
        settings.blog_id = None;
    }
    settings.save()?;
    if let Err(err) = api.logout(&session).await {
        eprintln!("warning: the temporary session was not revoked ({err}); it expires server-side");
    }
    println!("Logged in as {}.", me.email);
    println!("Token saved to {}.", Settings::path()?.display());
    Ok(())
}

async fn whoami(shared: &Shared) -> Result<(), CliError> {
    let authed = Authed::new(shared).await?;
    let blogs = authed.api.blogs(&authed.pat).await?;
    if authed.json {
        return print_json(&serde_json::json!({ "server": authed.server, "blogs": blogs }));
    }
    println!("server: {}", authed.server);
    println!("token: valid");
    if let Some(blog) = authed
        .blog
        .as_deref()
        .or(authed.settings.blog_id.as_deref())
    {
        println!("blog: {blog}");
    }
    if blogs.is_empty() {
        println!("blogs: none");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = blogs
        .iter()
        .map(|blog| vec![blog.slug.clone(), blog.name.clone()])
        .collect();
    print!("{}", output::table(&["slug", "name"], &rows));
    Ok(())
}

async fn logout(server: Option<&str>) -> Result<(), CliError> {
    let mut settings = Settings::load()?;
    let server = resolve_server(server, &settings);
    if let (Ok(server), Some(pat_id)) = (server, settings.pat_id.clone()) {
        match revoke_token(&server, &pat_id).await {
            Ok(()) => println!("Token revoked on the server."),
            Err(err) => eprintln!(
                "warning: token not revoked ({err}); delete it in the dashboard if it still works"
            ),
        }
    }
    let had_token = settings.pat.take().is_some();
    settings.pat_id = None;
    settings.save()?;
    println!(
        "{}",
        if had_token {
            "Local token cleared."
        } else {
            "No saved token; nothing to clear."
        }
    );
    Ok(())
}

/// Revocation is a session capability, so it asks for credentials once more.
async fn revoke_token(server: &str, pat_id: &str) -> Result<(), CliError> {
    if !std::io::stdin().is_terminal() {
        return Err(CliError::usage(
            "token revocation needs an interactive terminal",
        ));
    }
    let api = client::Api::new(server)?;
    let email = prompt("Email: ")?;
    let password = rpassword::prompt_password("Password: ").map_err(CliError::Io)?;
    let (_, session) = api.login(&email, &password).await?;
    let result = api.delete_pat(&session, pat_id).await;
    let _ = api.logout(&session).await;
    result
}

// Blog commands ---------------------------------------------------------

async fn blog_command(shared: &Shared, command: BlogCommand) -> Result<(), CliError> {
    let authed = Authed::new(shared).await?;
    match command {
        BlogCommand::List => {
            let blogs = authed.api.blogs(&authed.pat).await?;
            if authed.json {
                return print_json(&blogs);
            }
            if blogs.is_empty() {
                println!("No blogs yet. Create one: `statesu-cli blog create \"Name\"`");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = blogs
                .iter()
                .map(|blog| vec![blog.slug.clone(), blog.name.clone()])
                .collect();
            print!("{}", output::table(&["slug", "name"], &rows));
        }
        BlogCommand::Create { name, slug } => {
            let name = name.trim();
            if name.is_empty() {
                return Err(CliError::usage("blog name is required"));
            }
            let slug = match slug {
                Some(slug) => slug,
                None => slugify(name)
                    .ok_or_else(|| CliError::usage("cannot derive a URL slug: pass --slug"))?,
            };
            let blog = authed.api.create_blog(&authed.pat, name, &slug).await?;
            if authed.json {
                return print_json(&blog);
            }
            println!("Blog '{}' created.", blog.slug);
        }
        BlogCommand::Use { slug } => {
            let mut authed = authed;
            let blogs = authed.api.blogs(&authed.pat).await?;
            let found = blogs.iter().find(|blog| blog.slug == slug).ok_or_else(|| {
                CliError::usage(format!(
                    "no blog with slug '{slug}'; run `statesu-cli blog list`"
                ))
            })?;
            authed.settings.blog_id = Some(found.id.clone());
            authed.settings.save()?;
            println!("Now using blog '{}'.", found.name);
        }
    }
    Ok(())
}

// Post commands ---------------------------------------------------------

async fn post_command(shared: &Shared, command: PostCommand) -> Result<(), CliError> {
    let authed = Authed::new(shared).await?;
    match command {
        PostCommand::New {
            title,
            slug,
            publish,
        } => {
            let blog_id = authed.blog_id().await?;
            let title = check_title(&title)?;
            let slug = choose_slug(slug, title)?;
            let draft = editor::open("")?;
            if draft.content.trim().is_empty() {
                return Err(CliError::usage("empty post: nothing created"));
            }
            return create_post(
                &authed,
                &blog_id,
                &slug,
                title,
                draft.content.clone(),
                publish,
                Some(draft),
            )
            .await;
        }
        PostCommand::Create {
            title,
            file,
            slug,
            publish,
        } => {
            let blog_id = authed.blog_id().await?;
            let title = check_title(&title)?;
            let slug = choose_slug(slug, title)?;
            let content = read_content(file)?;
            return create_post(&authed, &blog_id, &slug, title, content, publish, None).await;
        }
        PostCommand::List { page, size } => {
            let blog_id = authed.blog_id().await?;
            let paged = authed
                .api
                .posts(&authed.pat, &blog_id, page.unwrap_or(1), size.unwrap_or(20))
                .await?;
            if authed.json {
                return print_json(&paged);
            }
            let rows: Vec<Vec<String>> = paged
                .items
                .iter()
                .map(|post| vec![post.slug.clone(), post.status.clone(), post.title.clone()])
                .collect();
            print!("{}", output::table(&["slug", "status", "title"], &rows));
            eprintln!(
                "showing {} of {} posts (page {})",
                paged.items.len(),
                paged.total,
                paged.page
            );
        }
        PostCommand::Get { target } => {
            let blog_id = authed.blog_id().await?;
            let post = authed.post(&blog_id, &target).await?;
            if authed.json {
                return print_json(&post);
            }
            print!("{}", post.content_markdown);
        }
        PostCommand::Edit { target } => {
            let blog_id = authed.blog_id().await?;
            let post = authed.post(&blog_id, &target).await?;
            if post.status == STATUS_PUBLISHED {
                eprintln!("warning: editing a published post changes the live content immediately");
            }
            let draft = editor::open(&post.content_markdown)?;
            if draft.content == post.content_markdown {
                draft.discard();
                println!("No changes.");
                return Ok(());
            }
            let editor::Draft { path, content } = draft;
            let patch = client::Patch {
                content_markdown: Some(content),
                ..Default::default()
            };
            let updated = match authed
                .api
                .patch_post(&authed.pat, &blog_id, &post.id, &patch)
                .await
            {
                Ok(updated) => {
                    let _ = std::fs::remove_file(&path);
                    updated
                }
                Err(err) => {
                    return Err(
                        err.with_note(&format!("your edited copy is kept at {}", path.display()))
                    );
                }
            };
            announce("Updated", &updated);
        }
        PostCommand::Publish { target } => {
            return set_status(&authed, &target, STATUS_PUBLISHED).await;
        }
        PostCommand::Unpublish { target } => {
            return set_status(&authed, &target, STATUS_DRAFT).await;
        }
        PostCommand::Delete { target, yes } => {
            let blog_id = authed.blog_id().await?;
            let post = authed.post(&blog_id, &target).await?;
            if !yes {
                if !std::io::stdin().is_terminal() {
                    return Err(CliError::usage(
                        "deletion requires --yes outside an interactive terminal",
                    ));
                }
                let answer = prompt(&format!(
                    "Delete post '{}' permanently? Type 'yes' to confirm: ",
                    post.slug
                ))?;
                if !answer.eq_ignore_ascii_case("yes") {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            authed
                .api
                .delete_post(&authed.pat, &blog_id, &post.id)
                .await?;
            announce("Deleted", &post);
        }
    }
    Ok(())
}

/// Shared create path for `new` (editor content) and `create` (file or
/// stdin): one validation, one request, one announcement, and the draft
/// recovery copy is kept until the server accepts.
async fn create_post(
    authed: &Authed,
    blog_id: &str,
    slug: &str,
    title: &str,
    content: String,
    publish: bool,
    draft: Option<editor::Draft>,
) -> Result<(), CliError> {
    check_content(&content)?;
    let status = publish.then_some(STATUS_PUBLISHED);
    match authed
        .api
        .create_post(&authed.pat, blog_id, slug, title, content.as_str(), status)
        .await
    {
        Ok(post) => {
            if let Some(draft) = draft {
                draft.discard();
            }
            announce(
                if publish {
                    "Published"
                } else {
                    "Created draft"
                },
                &post,
            );
            Ok(())
        }
        Err(err) => {
            let note = draft.map(|draft| format!("your text is kept at {}", draft.path.display()));
            Err(match note {
                Some(note) => err.with_note(&note),
                None => err,
            })
        }
    }
}

async fn set_status(authed: &Authed, target: &str, status: &str) -> Result<(), CliError> {
    let blog_id = authed.blog_id().await?;
    let post = authed.post(&blog_id, target).await?;
    let patch = client::Patch {
        status: Some(status.to_owned()),
        ..Default::default()
    };
    let _ = authed
        .api
        .patch_post(&authed.pat, &blog_id, &post.id, &patch)
        .await?;
    announce(
        if status == STATUS_PUBLISHED {
            "Published"
        } else {
            "Unpublished"
        },
        &post,
    );
    Ok(())
}

// Input and output helpers ---------------------------------------------

fn prompt(text: &str) -> Result<String, CliError> {
    let mut stderr = std::io::stderr().lock();
    eprint!("{text}");
    stderr.flush()?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(CliError::Io)?;
    Ok(line.trim().to_owned())
}

fn print_json(value: &impl serde::Serialize) -> Result<(), CliError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|err| CliError::Config(format!("cannot serialize output: {err}")))?;
    println!("{text}");
    Ok(())
}

fn announce(action: &str, post: &client::Post) {
    println!(
        "{} '{}' [{}] id {}",
        action, post.slug, post.status, post.id
    );
}

// Local field validation: these mirror the server limits so bad input fails
// before an upload round-trip.

fn check_title(title: &str) -> Result<&str, CliError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(CliError::usage("title is required"));
    }
    if trimmed.len() > MAX_TITLE_BYTES {
        return Err(CliError::usage("title too long: max 256 bytes"));
    }
    Ok(trimmed)
}

fn check_content(content: &str) -> Result<(), CliError> {
    if content.len() > MAX_CONTENT_BYTES {
        return Err(CliError::usage(
            "post body too long: the server accepts up to 256 KiB",
        ));
    }
    Ok(())
}

fn choose_slug(explicit: Option<String>, title: &str) -> Result<String, CliError> {
    let slug = match explicit {
        Some(slug) => slug,
        None => slugify(title).ok_or_else(|| {
            CliError::usage("cannot derive a URL slug from the title: pass --slug")
        })?,
    };
    if !valid_slug(&slug) {
        return Err(CliError::usage(format!(
            "invalid slug '{slug}': lowercase a-z, 0-9 and '-', max 63 chars"
        )));
    }
    Ok(slug)
}

/// Post body for `create`: `--file`, else piped stdin; a terminal stdin is
/// never stolen and points at `post new` instead.
fn read_content(file: Option<PathBuf>) -> Result<String, CliError> {
    let content = match file {
        Some(path) => std::fs::read_to_string(path).map_err(CliError::Io)?,
        None if !std::io::stdin().is_terminal() => {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(CliError::Io)?;
            buffer
        }
        None => {
            return Err(CliError::usage(
                "no content given: pass --file or pipe the post body on stdin",
            ));
        }
    };
    check_content(&content)?;
    Ok(content)
}

/// Derives a URL slug from a title: lowercase, ASCII alphanumerics and
/// hyphens, common Latin diacritics folded. Turkish İ/ı/ş/ğ are included.
fn slugify(title: &str) -> Option<String> {
    let mut slug = String::new();
    for character in title.to_lowercase().chars() {
        match fold(character) {
            Some(folded) => slug.push(folded),
            None if (character.is_whitespace() || character == '-' || character == '_')
                && !slug.is_empty()
                && !slug.ends_with('-') =>
            {
                slug.push('-');
            }
            None => {}
        }
    }
    let slug = slug.trim_matches('-');
    (!slug.is_empty() && slug.len() <= 63).then(|| slug.to_owned())
}

fn fold(character: char) -> Option<char> {
    let folded = match character {
        'à'..='å' | 'ā' => 'a',
        'ç' => 'c',
        'è'..='ë' | 'ē' => 'e',
        'ì'..='ï' | 'ī' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ō' => 'o',
        'ù'..='ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ğ' => 'g',
        'ı' => 'i',
        'ş' => 's',
        character if character.is_ascii_alphanumeric() => character,
        _ => return None,
    };
    Some(folded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_folds_and_trims() {
        assert_eq!(slugify("İlk Yazım").as_deref(), Some("ilk-yazim"));
        assert_eq!(slugify("Hello, World!").as_deref(), Some("hello-world"));
        assert_eq!(slugify("über café").as_deref(), Some("uber-cafe"));
        assert_eq!(slugify("--- __trims__ ---").as_deref(), Some("trims"));
        assert_eq!(slugify("  "), None);
        // Only stripped characters collapse to nothing.
        assert_eq!(slugify("emoji 🚀 only").as_deref(), Some("emoji-only"));
    }

    #[test]
    fn choose_slug_validates_against_server_rules() {
        assert_eq!(
            choose_slug(Some("custom".into()), "t").expect("deriveable"),
            "custom"
        );
        assert!(choose_slug(None, "İlk Yazım").is_ok());
        assert!(choose_slug(None, "🚀").is_err());
        assert!(choose_slug(Some("Invalid Slug".into()), "t").is_err());
        assert!(choose_slug(Some("-bad-".into()), "t").is_err());
    }

    #[test]
    fn parse_uuid_accepts_canonicalization_only() {
        let parsed = parse_uuid("018F1C33-3F7C-7E13-B1b7-9AA5C623BCF9");
        assert!(parsed.is_ok());
        assert!(parse_uuid("not-a-uuid").is_err());
    }
}
