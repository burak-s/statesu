import { defineConfig, devices } from '@playwright/test';

const apiPort = process.env.STATESU_BROWSER_API_PORT ?? '4180';
const hostPort = process.env.STATESU_BROWSER_HOST_PORT ?? '4181';

export default defineConfig({
	testDir: './tests/browser',
	forbidOnly: true,
	retries: 0,
	workers: 1,
	timeout: 30_000,
	use: {
		baseURL: `http://localhost:${apiPort}`,
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure'
	},
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
	webServer: {
		command: 'node tests/browser-server.js',
		url: `http://127.0.0.1:${hostPort}/__test/ready`,
		timeout: 180_000,
		reuseExistingServer: false,
		gracefulShutdown: { signal: 'SIGTERM', timeout: 10_000 }
	}
});
