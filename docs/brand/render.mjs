import sharp from 'sharp';
const size = Number(process.argv[4] || 256);
const [input, output] = process.argv.slice(2);
await sharp(input, { density: 512 }).resize(size, size).png().toFile(output);
console.log('rendered', output);
