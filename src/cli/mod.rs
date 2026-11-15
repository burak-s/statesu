//! The `stscli` terminal client: create, edit, publish and delete posts
//! on a running Statesu server over its HTTP API. There is deliberately no
//! Git and no local post storage: the server is the source of truth, and
//! locally only connection settings and the saved token persist.

mod browser;
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
const DEFAULT_SERVER: &str = "https://statesu.com";
const TOKEN_ENV: &str = "STATESU_TOKEN";

/// Flags shared by every subcommand.
#[derive(Debug, clap::Args)]
pub struct Shared {
    /// CMS base URL, overriding STATESU_SERVER and saved settings (default: https://statesu.com).
    #[arg(long, global = true)]
    pub server: Option<String>,
    /// Blog id or slug for this command, overriding the stored selection.
    #[arg(long, global = true)]
    pub blog: Option<String>,
    /// Print machine-readable JSON on stdout.
    #[arg(long, global = true)]
    pub json: bool,
}

/// Top-level CLI shared by `stscli` and the legacy `statesu-cli` binary.
#[derive(Debug, Parser)]
#[command(
    name = "stscli",
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
    /// Upload and manage media images (posts reference them by URL).
    #[command(subcommand)]
    Media(MediaCommand),
}

#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    /// Upload one image; prints the Markdown reference it produced.
    Upload {
        /// Path of the image file (JPEG, PNG or WebP).
        file: PathBuf,
    },
    /// List uploaded media, newest first.
    List {
        /// 1-based page number [default: 1].
        #[arg(long)]
        page: Option<u32>,
        /// Page size, capped at 100 by the server [default: 20].
        #[arg(long)]
        size: Option<u32>,
    },
    /// Delete one media image (confirmation unless --yes). The id may be
    /// the media id itself or a pasted public URL.
    Delete {
        target: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum BlogCommand {
    /// Read, replace or reset the blog's shared embed styling (Markdown is unchanged).
    #[command(subcommand)]
    Appearance(AppearanceCommand),
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
pub enum AppearanceCommand {
    /// Print the saved appearance JSON (null means built-in defaults).
    Get,
    /// Replace the entire appearance with a versioned JSON document.
    Set {
        #[arg(long)]
        file: PathBuf,
    },
    /// Clear all saved styling and restore built-in defaults.
    Reset {
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Save the current styling under a name for later reuse.
    Save {
        /// Name of the saved style.
        name: String,
        /// Save this appearance document instead of the current styling.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// List saved styles.
    List,
    /// Apply a saved style to the blog.
    Apply {
        /// Name or id of the saved style.
        target: String,
    },
    /// Delete a saved style (confirmation unless --yes).
    Delete {
        /// Name or id of the saved style.
        target: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
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
            Command::Media(command) => media_command(shared, command).await,
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
        let server = resolve_server(shared.server.as_deref(), &settings);
        let api = client::Api::new(&server)?;
        let pat = env::var(TOKEN_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or(settings.pat.clone())
            .ok_or_else(|| {
                CliError::usage("no token saved: run `stscli login` or set STATESU_TOKEN")
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
                CliError::usage("no blog selected: run `stscli blog use <slug>` or pass --blog")
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

/// Resolves a saved style by name (case-insensitive) or id.
fn resolve_variant(
    variants: &[client::AppearanceVariant],
    target: &str,
) -> Result<client::AppearanceVariant, CliError> {
    variants
        .iter()
        .find(|variant| variant.name.eq_ignore_ascii_case(target) || variant.id == target)
        .cloned()
        .ok_or_else(|| {
            CliError::usage(format!(
                "no style '{target}'; run `stscli blog appearance list`"
            ))
        })
}

/// Parses a canonical UUID, trimmed; anything else is meant as a slug.
fn parse_uuid(text: &str) -> Result<String, ()> {
    text.trim()
        .parse::<uuid::Uuid>()
        .map(|uuid| uuid.to_string())
        .map_err(|_| ())
}

/// Server precedence: --server flag, STATESU_SERVER, stored settings, hosted default.
fn resolve_server(explicit: Option<&str>, settings: &Settings) -> String {
    explicit
        .map(str::to_owned)
        .or_else(|| {
            env::var(SERVER_ENV)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| settings.server.clone())
        .unwrap_or_else(|| DEFAULT_SERVER.to_owned())
}

// Auth commands ---------------------------------------------------------

async fn login(server: Option<&str>) -> Result<(), CliError> {
    let settings = Settings::load()?;
    let server = resolve_server(server, &settings);
    let api = client::Api::new(&server)?;
    let token = browser::login(&api, &server).await?;
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
    println!("Logged in via browser.");
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
    if let Some(pat) = settings.pat.as_deref() {
        let result = match client::Api::new(&server) {
            Ok(api) => api.revoke_token(pat).await,
            Err(err) => Err(err),
        };
        match result {
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

// Blog commands ---------------------------------------------------------

async fn blog_command(shared: &Shared, command: BlogCommand) -> Result<(), CliError> {
    let authed = Authed::new(shared).await?;
    match command {
        BlogCommand::Appearance(command) => {
            let blog_id = authed.blog_id().await?;
            match command {
                AppearanceCommand::Get => {
                    let blogs = authed.api.blogs(&authed.pat).await?;
                    let blog = blogs
                        .iter()
                        .find(|blog| blog.id == blog_id)
                        .ok_or_else(|| CliError::usage("blog not found"))?;
                    return print_json(&blog.embed_appearance);
                }
                AppearanceCommand::Set { file } => {
                    let raw = std::fs::read_to_string(file).map_err(CliError::Io)?;
                    let mut document: serde_json::Value =
                        serde_json::from_str(&raw).map_err(|_| {
                            CliError::usage(
                                "invalid appearance JSON: expected a versioned appearance document",
                            )
                        })?;
                    // Keyword defaults are the same contract the API applies;
                    // a file of only defaults clears the appearance.
                    crate::blog::appearance::strip_default_sentinels(&mut document);
                    if crate::blog::appearance::is_default_document(&document) {
                        let blog = authed
                            .api
                            .set_appearance(&authed.pat, &blog_id, None)
                            .await?;
                        return print_json(&blog.embed_appearance);
                    }
                    crate::blog::appearance::drop_empty_groups(&mut document);
                    let mut appearance: crate::blog::appearance::EmbedAppearance =
                        serde_json::from_value(document).map_err(|_| {
                            CliError::usage(
                                "invalid appearance JSON: expected a versioned appearance document",
                            )
                        })?;
                    appearance.validate().map_err(|_| {
                        CliError::usage(
                            "invalid appearance: check version, hex colors and numeric ranges",
                        )
                    })?;
                    let blog = authed
                        .api
                        .set_appearance(&authed.pat, &blog_id, Some(&appearance))
                        .await?;
                    return print_json(&blog.embed_appearance);
                }
                AppearanceCommand::Reset { yes } => {
                    if !yes {
                        if !std::io::stdin().is_terminal() {
                            return Err(CliError::usage(
                                "reset requires --yes outside an interactive terminal",
                            ));
                        }
                        if !prompt("Reset this blog's appearance? Type 'yes' to confirm: ")?
                            .eq_ignore_ascii_case("yes")
                        {
                            return Ok(());
                        }
                    }
                    let blog = authed
                        .api
                        .set_appearance(&authed.pat, &blog_id, None)
                        .await?;
                    return print_json(&blog.embed_appearance);
                }
                AppearanceCommand::Save { name, file } => {
                    let name = name.trim();
                    if name.is_empty() {
                        return Err(CliError::usage("style name is required"));
                    }
                    let document = match file {
                        Some(path) => {
                            let raw = std::fs::read_to_string(path).map_err(CliError::Io)?;
                            Some(serde_json::from_str::<serde_json::Value>(&raw)
                                .map_err(|_| {
                                    CliError::usage(
                                        "invalid appearance JSON: expected a versioned appearance document",
                                    )
                                })?)
                        }
                        None => None, // The server snapshots the current styling.
                    };
                    let variant = authed
                        .api
                        .create_variant(&authed.pat, &blog_id, name, document.as_ref())
                        .await?;
                    if authed.json {
                        return print_json(&variant);
                    }
                    println!("Style '{}' saved.", variant.name);
                }
                AppearanceCommand::List => {
                    let variants = authed.api.variants(&authed.pat, &blog_id).await?;
                    if authed.json {
                        return print_json(&variants);
                    }
                    if variants.is_empty() {
                        println!(
                            "No saved styles yet. Save one: `stscli blog appearance save <name>`"
                        );
                        return Ok(());
                    }
                    let rows: Vec<Vec<String>> = variants
                        .iter()
                        .map(|variant| vec![variant.name.clone(), variant.id.clone()])
                        .collect();
                    print!("{}", output::table(&["name", "id"], &rows));
                }
                AppearanceCommand::Apply { target } => {
                    let variants = authed.api.variants(&authed.pat, &blog_id).await?;
                    let variant = resolve_variant(&variants, &target)?;
                    let appearance = variant.appearance.clone().ok_or_else(|| {
                        CliError::usage(format!("style '{}' has no document", variant.name))
                    })?;
                    let mut document = appearance;
                    crate::blog::appearance::strip_default_sentinels(&mut document);
                    crate::blog::appearance::drop_empty_groups(&mut document);
                    let mut appearance: crate::blog::appearance::EmbedAppearance =
                        serde_json::from_value(document).map_err(|_| {
                            CliError::usage("saved style is not a valid appearance document")
                        })?;
                    appearance.validate().map_err(|_| {
                        CliError::usage("saved style is not a valid appearance document")
                    })?;
                    let blog = authed
                        .api
                        .set_appearance(&authed.pat, &blog_id, Some(&appearance))
                        .await?;
                    if authed.json {
                        return print_json(&blog.embed_appearance);
                    }
                    println!("Style '{}' applied.", variant.name);
                }
                AppearanceCommand::Delete { target, yes } => {
                    let variants = authed.api.variants(&authed.pat, &blog_id).await?;
                    let variant = resolve_variant(&variants, &target)?;
                    if !yes {
                        if !std::io::stdin().is_terminal() {
                            return Err(CliError::usage(
                                "deletion requires --yes outside an interactive terminal",
                            ));
                        }
                        if !prompt(&format!(
                            "Delete style '{}' permanently? Type 'yes' to confirm: ",
                            variant.name
                        ))?
                        .eq_ignore_ascii_case("yes")
                        {
                            return Ok(());
                        }
                    }
                    authed
                        .api
                        .delete_variant(&authed.pat, &blog_id, &variant.id)
                        .await?;
                    if authed.json {
                        return print_json(&variant);
                    }
                    println!("Style '{}' deleted.", variant.name);
                }
            }
        }
        BlogCommand::List => {
            let blogs = authed.api.blogs(&authed.pat).await?;
            if authed.json {
                return print_json(&blogs);
            }
            if blogs.is_empty() {
                println!("No blogs yet. Create one: `stscli blog create \"Name\"`");
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
                    "no blog with slug '{slug}'; run `stscli blog list`"
                ))
            })?;
            authed.settings.blog_id = Some(found.id.clone());
            authed.settings.save()?;
            println!("Now using blog '{}'.", found.name);
        }
    }
    Ok(())
}

// Media commands ---------------------------------------------------------

fn media_markdown(media: &client::Media, source_name: Option<&str>) -> String {
    let url = match media.public_url.as_deref() {
        Some(url) if !url.is_empty() => url.to_owned(),
        _ => return String::new(),
    };
    let base = source_name
        .map(|name| {
            std::path::Path::new(name)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("")
                .to_owned()
        })
        .unwrap_or_default()
        .trim()
        .replace('[', "(")
        .replace(']', ")");
    let alt: &str = if base.is_empty() {
        "image"
    } else {
        base.as_str()
    };
    format!("![{alt}]({url})")
}

/// Accepts the media id itself or a pasted public URL whose object path is
/// `blogs/{blog}/{media}/{label}.{ext}`; every other shape is a usage error.
fn media_id_from_target(target: &str) -> Result<String, CliError> {
    if let Ok(id) = parse_uuid(target) {
        return Ok(id);
    }
    let position = target.find("/blogs/").ok_or_else(|| {
        CliError::usage(
            "media target must be the media id or its public URL containing /blogs/<blog>/<media>/",
        )
    })?;
    let after = &target[position + 7..];
    let media_part = after.split_once('/').map(|(_, rest)| rest).unwrap_or("");
    let id = media_part
        .split(['/'])
        .next()
        .ok_or_else(|| CliError::usage("cannot read the media id from that URL"))?;
    parse_uuid(id).map_err(|()| CliError::usage("cannot read the media id from that URL"))
}

fn human_bytes(total: i64) -> String {
    if total < 1024 {
        return format!("{total} B");
    }
    if total < 1024 * 1024 {
        return format!("{:.1} KiB", total as f64 / 1024.0);
    }
    format!("{:.1} MiB", total as f64 / 1024.0 / 1024.0)
}

async fn media_command(shared: &Shared, command: MediaCommand) -> Result<(), CliError> {
    let authed = Authed::new(shared).await?;
    let blog_id = authed.blog_id().await?;
    match command {
        MediaCommand::Upload { file } => {
            let source_name = file
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned);
            let bytes = std::fs::read(&file).map_err(|err| {
                CliError::usage(format!("cannot read '{}': {err}", file.display()))
            })?;
            let media = authed
                .api
                .upload_media(&authed.pat, &blog_id, bytes)
                .await?;
            let markdown = media_markdown(&media, source_name.as_deref());
            if authed.json {
                return print_json(&serde_json::json!({
                    "media": media,
                    "markdown": markdown,
                }));
            }
            match (media.width, media.height) {
                (Some(width), Some(height)) => {
                    println!("stored: {} {}x{} ({})", media.id, width, height, media.mime);
                }
                _ => println!("stored: {} ({})", media.id, media.mime),
            }
            println!(
                "href:   {}",
                media.public_url.as_deref().unwrap_or("(no public url)")
            );
            if markdown.is_empty() {
                println!("note:   media has no public URL; paste it manually");
            } else {
                println!("markdown: {markdown}");
            }
        }
        MediaCommand::List { page, size } => {
            let page = page.unwrap_or(1);
            let size = size.unwrap_or(crate::pagination::DEFAULT_PAGE_SIZE);
            let listed = authed
                .api
                .media_list(&authed.pat, &blog_id, page, size)
                .await?;
            if authed.json {
                return print_json(&listed);
            }
            if listed.items.is_empty() {
                println!("No media yet. Upload one: `stscli media upload image.jpg`");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = listed
                .items
                .iter()
                .map(|media| {
                    let dimensions = match (media.width, media.height) {
                        (Some(width), Some(height)) => format!("{width}x{height}"),
                        _ => "-".to_owned(),
                    };
                    vec![
                        media.id.clone(),
                        media.mime.clone(),
                        dimensions,
                        human_bytes(media.stored_bytes),
                        media.public_url.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            print!(
                "{}",
                output::table(&["id", "mime", "size", "bytes", "public url"], &rows)
            );
        }
        MediaCommand::Delete { target, yes } => {
            let media_id = media_id_from_target(&target)?;
            if !yes {
                print!("Delete media {media_id}? This cannot be undone [y/N] ");
                std::io::stdout()
                    .flush()
                    .map_err(|err| CliError::Config(format!("cannot prompt: {err}")))?;
                let mut answer = String::new();
                std::io::stdin()
                    .read_line(&mut answer)
                    .map_err(|err| CliError::Config(format!("cannot read answer: {err}")))?;
                let answer = answer.trim().to_ascii_lowercase();
                if answer != "y" && answer != "yes" {
                    println!("aborted");
                    return Ok(());
                }
            }
            authed
                .api
                .delete_media(&authed.pat, &blog_id, &media_id)
                .await?;
            println!("Media {media_id} deleted.");
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
    // Announce with the patched response: the pre-patch record still
    // carries the old status and would label a publish as a draft.
    let patched = authed
        .api
        .patch_post(&authed.pat, &blog_id, &post.id, &patch)
        .await?;
    announce(
        if patched.status == STATUS_PUBLISHED {
            "Published"
        } else {
            "Unpublished"
        },
        &patched,
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
    fn command_name_is_stscli_and_legacy_invocation_still_parses() {
        use clap::CommandFactory;
        let mut command = Cli::command();
        assert_eq!(command.get_name(), "stscli");
        assert!(command.render_help().to_string().contains("Usage: stscli"));
        for binary in ["stscli", "statesu-cli"] {
            let cli = Cli::try_parse_from([binary, "login"]).unwrap();
            assert!(matches!(cli.command, Command::Login));
        }
    }

    #[test]
    fn server_resolution_defaults_to_hosted_and_preserves_overrides() {
        let mut settings = Settings::default();
        let environment = env::var(SERVER_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty());
        assert_eq!(
            resolve_server(None, &settings),
            environment.as_deref().unwrap_or("https://statesu.com")
        );
        settings.server = Some("https://saved.example".into());
        assert_eq!(
            resolve_server(None, &settings),
            environment.as_deref().unwrap_or("https://saved.example")
        );
        assert_eq!(
            resolve_server(Some("https://custom.example"), &settings),
            "https://custom.example"
        );
    }

    #[test]
    fn login_uses_browser_and_rejects_token_flag() {
        let cli = Cli::try_parse_from([
            "stscli",
            "login",
            "--server",
            "https://statesu.com",
            "--token",
        ]);
        assert!(cli.is_err(), "manual token login is no longer supported");
        assert!(
            Cli::try_parse_from(["stscli", "login", "--server", "https://statesu.com"]).is_ok()
        );
    }

    #[test]
    fn appearance_commands_parse() {
        for args in [
            vec!["stscli", "blog", "appearance", "get", "--blog", "example"],
            vec![
                "stscli",
                "blog",
                "appearance",
                "set",
                "--file",
                "appearance.json",
            ],
            vec!["stscli", "blog", "appearance", "reset", "--yes"],
            vec!["stscli", "blog", "appearance", "save", "cozy"],
            vec![
                "stscli",
                "blog",
                "appearance",
                "save",
                "cozy",
                "--file",
                "a.json",
            ],
            vec!["stscli", "blog", "appearance", "list"],
            vec!["stscli", "blog", "appearance", "apply", "cozy"],
            vec!["stscli", "blog", "appearance", "delete", "cozy", "--yes"],
        ] {
            assert!(matches!(
                Cli::try_parse_from(args).unwrap().command,
                Command::Blog(BlogCommand::Appearance(_))
            ));
        }
        assert!(Cli::try_parse_from(["stscli", "blog", "appearance", "set"]).is_err());
        assert!(Cli::try_parse_from(["stscli", "blog", "appearance", "save"]).is_err());
    }

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
