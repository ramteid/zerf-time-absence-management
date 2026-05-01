// Headless smoke-test of the live homepage via puppeteer.
const puppeteer = require('puppeteer');
(async () => {
  const url = process.env.URL || 'https://REDACTED_DOMAIN/';
  const browser = await puppeteer.launch({ args: ['--no-sandbox','--disable-setuid-sandbox'] });
  const page = await browser.newPage();
  const errors = [], logs = [], fails = [];
  page.on('console', m => logs.push(`[${m.type()}] ${m.text()}`));
  page.on('pageerror', e => errors.push(`pageerror: ${e.message}`));
  page.on('requestfailed', r => fails.push(`failed ${r.url()} -> ${r.failure()?.errorText ?? 'unknown'}`));
  const resp = await page.goto(url, { waitUntil: 'networkidle0', timeout: 20000 });
  console.log('HTTP', resp.status());
  await new Promise(r => setTimeout(r, 1000));
  const root = await page.$eval('#app', el => el.innerHTML.slice(0, 400));
  const title = await page.title();
  console.log('title:', title);
  console.log('#app innerHTML[0..400]:');
  console.log(root);
  console.log('--- console logs ---'); logs.forEach(l => console.log(l));
  console.log('--- page errors ---'); errors.forEach(l => console.log(l));
  console.log('--- failed requests ---'); fails.forEach(l => console.log(l));
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
