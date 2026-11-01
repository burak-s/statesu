<script>
	import Terminal from '#lib/components/Terminal.svelte';

	const install = `git clone https://github.com/burak-s/statesu.git
cd statesu
cargo build --release --locked --bin stscli

# Linux / macOS: ensure ~/.local/bin is on your PATH.
mkdir -p ~/.local/bin
cp target/release/stscli ~/.local/bin/
stscli --help`;
	const token = `export STATESU_SERVER=https://statesu.com
# Bash: enter a dashboard token without saving it in shell history.
read -r -s -p 'API token: ' STATESU_TOKEN; printf '\\n'
export STATESU_TOKEN
stscli whoami`;
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
	<p>On Windows, copy <code>target/release/stscli.exe</code> to a directory on your user PATH instead. To update, pull the repository, rebuild and replace the installed binary.</p>
</section>

<section>
	<h2>2. Connect your account</h2>
	<p><a href="/dashboard">Register in the dashboard</a> and verify your email. Run login to open the website, sign in (including Turnstile when enabled), and approve CLI access. Your browser returns a one-time code to the CLI, which securely exchanges it for a write-capable token and saves it in your user configuration directory. No password or token is entered in the terminal.</p>
	<Terminal code={'stscli login\nstscli whoami'} />
	<p>The legacy <code>statesu-cli</code> command remains available for compatibility. Both commands share the same saved connection settings.</p>
	<p>The default server is <code>https://statesu.com</code>. For a self-hosted deployment, pass <code>--server https://your-host</code> or set <code>STATESU_SERVER</code>. Login remembers the chosen server.</p>
	<p>If the browser does not open, visit the printed URL on the same computer within five minutes. Only approve requests you started yourself. SSH use requires forwarding the printed loopback port; otherwise use a provisioned token for automation.</p>
	<h3>Automation</h3>
	<p>For headless tools, create a write-capable token in the dashboard's <strong>API tokens</strong> and use an environment variable without saving it to disk:</p>
	<Terminal code={token} />
	<p>This environment-based connection lasts for the current shell session. Keep your token secret; for automation, use your secret manager.</p>
</section>

<section>
	<h2>3. Choose a blog</h2>
	<Terminal code={'stscli blog create "Writing desk" --slug writing-desk\nstscli blog list\nstscli blog use writing-desk'} />
	<p>The selected blog is saved for future commands. Use <code>--blog writing-desk</code> to override it for one command.</p>
</section>

<section>
	<h2>4. Write and publish</h2>
	<p>Set <code>VISUAL</code> or <code>EDITOR</code> to your preferred editor. New posts are drafts unless you add <code>--publish</code>.</p>
	<Terminal code={'stscli post new "Hello world" --slug hello-world\nstscli post list\nstscli post publish hello-world'} />
	<p>Upload a file or pipe Markdown instead of opening an editor:</p>
	<Terminal code={'stscli post create "From a file" --file post.md --publish\nprintf \'# Hello\\n\\nFirst post.\\n\' | stscli post create "First post" --publish'} />
	<p>Ready to show it on your website? Follow the <a href="/docs/integration">web integration guide</a>.</p>
</section>

<section>
	<h2>Edit and manage</h2>
	<Terminal code={'stscli post get hello-world\nstscli post edit hello-world\nstscli post unpublish hello-world\nstscli post delete hello-world'} />
	<p>Commands accept a post ID or slug. Editing a published post changes it live; there is no revision history. Unpublishing returns it to draft. Deleting asks for confirmation; <code>--yes</code> skips the prompt. Deleted slugs stay reserved.</p>
	<h3>Automation</h3>
	<p>Set <code>STATESU_SERVER</code> and <code>STATESU_TOKEN</code> through your environment or secret manager. Use <code>--json</code> for machine-readable output.</p>
	<Terminal code={'stscli --blog writing-desk post create "Release notes" --file notes.md --publish\nstscli --blog writing-desk --json post list'} />
	<p>Run <code>stscli --help</code> or <code>stscli post --help</code> for all options. <code>stscli logout</code> revokes the saved token and clears local settings without asking for your password. Revoke environment-provided tokens in the dashboard.</p>
</section>
