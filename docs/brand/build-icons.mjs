// 从 logo-v2.svg 生成 Tauri 全部图标资产：各尺寸 PNG、.icns、.ico、Windows 竖版、iOS/Android。
// 运行：node build-icons.mjs（需 iconutil、sharp、png-to-ico）
import sharp from 'sharp';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import pngToIco from 'png-to-ico';
import { fileURLToPath } from 'node:url';

const root = path.dirname(fileURLToPath(import.meta.url));
const svg = path.join(root, 'logo-v2.svg');
const out = path.join(root, 'out');
await fs.rm(out, { recursive: true, force: true });
await fs.mkdir(out, { recursive: true });

const render = (px, file, opts = {}) =>
  sharp(svg, { density: Math.max(72, (px / 512) * 512) }).resize(px, px).png(opts).toFile(path.join(out, file));

// Tauri 桌面 PNG
await Promise.all([30, 32, 48, 64, 128, 256].map(px => render(px, `${px}x${px}.png`)));
await render(128, '128x128@2x.png').then(() => fs.rename(path.join(out, '128x128@2x.png'), path.join(out, '128x128@2x.png'))).catch(() => {});
await sharp(svg, { density: 512 }).resize(256, 256).png().toFile(path.join(out, '128x128@2x.png'));
await render(512, 'icon.png');
await render(256, 'Square296x296Logo.png');

// macOS .icns：icon_16..512_@2x
const sizes = [16, 32, 64, 128, 256, 512];
const icnsDir = path.join(out, 'icns.iconset');
await fs.mkdir(icnsDir, { recursive: true });
for (const s of sizes) {
  await sharp(svg, { density: 1024 }).resize(s, s).png().toFile(path.join(icnsDir, `icon_${s}x${s}.png`));
  await sharp(svg, { density: 1024 }).resize(s * 2, s * 2).png().toFile(path.join(icnsDir, `icon_${s}x${s}@2x.png`));
}
execFileSync('iconutil', ['-c', 'icns', icnsDir, '-o', path.join(out, 'icon.icns')]);

// Windows .ico
const icoSizes = [16, 24, 32, 48, 64, 128, 256];
const buffers = [];
for (const s of icoSizes) buffers.push(await sharp(svg, { density: 1024 }).resize(s, s).png().toBuffer());
await fs.writeFile(path.join(out, 'icon.ico'), await pngToIco(buffers));

// 简单的移动端占位（Tauri 要求存在即可，本项目桌面优先）
await fs.copyFile(path.join(out, 'icon.png'), path.join(out, 'StoreLogo.png'));

console.log('icons generated in', out);
