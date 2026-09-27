import {defineConfig} from '@playwright/test';
export default defineConfig({testDir:'./tests',testMatch:'**/*.e2e.mjs',timeout:30_000,use:{browserName:'chromium',headless:true}});
