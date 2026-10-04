// Minimal 24-bit BMP encoder and header reader for the installer images.
// NSIS Modern UI and WiX only accept uncompressed BMP, which no image library
// in this repository writes, and the format is small enough to do by hand.

/** Encodes RGBA pixels (alpha ignored) as an uncompressed 24-bit bottom-up BMP. */
export function encodeBmp24(width, height, rgba) {
  if (rgba.length !== width * height * 4) {
    throw new Error(`expected ${width * height * 4} bytes of RGBA, got ${rgba.length}`);
  }
  const stride = Math.ceil((width * 3) / 4) * 4; // rows are padded to 4 bytes
  const pixelBytes = stride * height;
  const out = Buffer.alloc(54 + pixelBytes);
  out.write("BM", 0, "ascii");
  out.writeUInt32LE(out.length, 2); // file size
  out.writeUInt32LE(54, 10); // offset to the pixel data
  out.writeUInt32LE(40, 14); // BITMAPINFOHEADER
  out.writeInt32LE(width, 18);
  out.writeInt32LE(height, 22); // positive: rows run bottom to top
  out.writeUInt16LE(1, 26); // planes
  out.writeUInt16LE(24, 28); // bits per pixel
  out.writeUInt32LE(0, 30); // BI_RGB, no compression
  out.writeUInt32LE(pixelBytes, 34);
  out.writeInt32LE(2835, 38); // 72 dpi
  out.writeInt32LE(2835, 42);
  for (let y = 0; y < height; y += 1) {
    let at = 54 + (height - 1 - y) * stride;
    for (let x = 0; x < width; x += 1) {
      const p = (y * width + x) * 4;
      out[at++] = rgba[p + 2]; // BMP stores BGR
      out[at++] = rgba[p + 1];
      out[at++] = rgba[p];
    }
  }
  return out;
}

/** Reads what NSIS and WiX care about from a BMP header; throws if it is not a BMP. */
export function readBmpInfo(buffer) {
  if (buffer.length < 54 || buffer.toString("ascii", 0, 2) !== "BM") {
    throw new Error("not a BMP file");
  }
  return {
    width: buffer.readInt32LE(18),
    height: buffer.readInt32LE(22),
    bitsPerPixel: buffer.readUInt16LE(28),
    compression: buffer.readUInt32LE(30),
    size: buffer.readUInt32LE(2),
  };
}
