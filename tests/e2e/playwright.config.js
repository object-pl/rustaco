import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './specs',
  fullyParallel: true,
  reporter: process.env.CI ? 'line' : 'list',
  use: { baseURL: 'http://127.0.0.1:47816/' },
  webServer: [
    { command: 'node serve.mjs', url: 'http://127.0.0.1:47816/', reuseExistingServer: !process.env.CI },
    { command: 'node lsp-server.mjs', port: 47817, reuseExistingServer: !process.env.CI },
  ],
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
