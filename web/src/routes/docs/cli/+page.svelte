<script>
	import Terminal from '#lib/components/Terminal.svelte';

	const install = `git clone https://github.com/burak-s/statesu.git
cd statesu
cargo build --release --locked --bin statesu-cli

# Linux / macOS: ensure ~/.local/bin is on your PATH.
mkdir -p ~/.local/bin
cp target/release/statesu-cli ~/.local/bin/
statesu-cli --help`;
	const token = `export STATESU_SERVER=https://statesu.com
# Bash: enter a dashboard token without saving it in shell history.
read -r -s -p 'API token: ' STATESU_TOKEN; printf '\\n'
export STATESU_TOKEN
statesu-cli whoami`;
</script>

<svelte:head>
	<title>CLI guide — Statesu</title>
	<meta name="description" content="Install the Statesu CLI, connect your account, and write and publish Markdown from your terminal." />
	<link rel="canonical" href="https://statesu.com/docs/cli" />
</svelte:head>

<section class="intro">
	<p class="eyebrow">CLI guide</p>
	<h1>Write from your terminal.</h1>
	<p class="lead">Use your favorite editor. Save drafts and publish without Git or a local content repository. Your posts live on the server.</p>
</section>

<section>
	<h2>1. Install</h2>
	<p>Install <a href="https://git-scm.com/">Git</a> and <a href="https://rustup.rs/">Rust with rustup</a>, then build from source. Rustup selects the repository's pinned toolchain automatically. You do not need to run a server.</p>
	<Terminal code={install} />
	<p>On Windows, copy <code>target/release/statesu-cli.exe</code> to a directory on your user PATH instead. To update, pull the repository, rebuild and replace the installed binary.</p>
</section>

<section>
	<h2>2. Connect your account</h2>
	<p><a href="/dashboard">Register in the dashboard</a> and verify your email. Then sign in from an interactive terminal; the CLI prompts for your credentials and saves a write-capable token in your user configuration directory.</p>
	<Terminal code={'statesu-cli login --server https://statesu.com\nstatesu-cli whoami'} />
	<h3>Using a dashboard token</h3>
	<p>If Turnstile is enabled, CLI password login cannot complete the browser challenge. Sign in to the dashboard and create a token with write access in <strong>API tokens</strong>, then use it instead:</p>
	<Terminal code={token} />
	<p>This environment-based connection lasts for the current shell session. Keep your token secret; for automation, use your secret manager.</p>
</section>

<section>
	<h2>3. Choose a blog</h2>
	<Terminal code={'statesu-cli blog create "Writing desk" --slug writing-desk\nstatesu-cli blog list\nstatesu-cli blog use writing-desk'} />
	<p>The selected blog is saved for future commands. Use <code>--blog writing-desk</code> to override it for one command.</p>
</section>

<section>
	<h2>4. Write and publish</h2>
	<p>Set <code>VISUAL</code> or <code>EDITOR</code> to your preferred editor. New posts are drafts unless you add <code>--publish</code>.</p>
	<Terminal code={'statesu-cli post new "Hello world" --slug hello-world\nstatesu-cli post list\nstatesu-cli post publish hello-world'} />
	<p>Upload a file or pipe Markdown instead of opening an editor:</p>
	<Terminal code={'statesu-cli post create "From a file" --file post.md --publish\nprintf \'# Hello\\n\\nFirst post.\\n\' | statesu-cli post create "First post" --publish'} />
	<p>Ready to show it on your website? Follow the <a href="/docs/integration">web integration guide</a>.</p>
</section>

<section>
	<h2>Edit and manage</h2>
	<Terminal code={'statesu-cli post get hello-world\nstatesu-cli post edit hello-world\nstatesu-cli post unpublish hello-world\nstatesu-cli post delete hello-world'} />
	<p>Commands accept a post ID or slug. Editing a published post changes it live; there is no revision history. Unpublishing returns it to draft. Deleting asks for confirmation; <code>--yes</code> skips the prompt. Deleted slugs stay reserved.</p>
	<h3>Automation</h3>
	<p>Set <code>STATESU_SERVER</code> and <code>STATESU_TOKEN</code> through your environment or secret manager. Use <code>--json</code> for machine-readable output.</p>
	<Terminal code={'statesu-cli --blog writing-desk post create "Release notes" --file notes.md --publish\nstatesu-cli --blog writing-desk --json post list'} />
	<p>Run <code>statesu-cli --help</code> or <code>statesu-cli post --help</code> for all options. <code>statesu-cli logout</code> revokes an interactively saved token and clears local settings; revoke dashboard-created tokens in the dashboard.</p>
</section>
