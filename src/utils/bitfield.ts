/**
 * Reading rtorrent's piece bitfield (`d.bitfield`).
 *
 * The bitfield is a hex string where each byte (two hex chars) covers 8 pieces,
 * most-significant bit first — i.e. piece `i` lives in byte `i >> 3` at bit
 * `7 - (i & 7)`. This is the standard BitTorrent bitfield layout.
 *
 * These helpers are pure so the bit ordering (easy to get subtly wrong) is
 * unit-tested against real rtorrent output — see bitfield.test.ts.
 */

/** Decode the hex bitfield into bytes. Non-hex/odd input yields what parses. */
export function bitfieldToBytes(hex: string): Uint8Array {
  const clean = hex.trim();
  const len = clean.length >> 1; // ignore a trailing half-byte
  const out = new Uint8Array(len);
  for (let i = 0; i < len; i++) {
    const byte = Number.parseInt(clean.substr(i * 2, 2), 16);
    out[i] = Number.isNaN(byte) ? 0 : byte;
  }
  return out;
}

/** Is piece `index` present in this bitfield? Out-of-range reads as false. */
export function hasPiece(bytes: Uint8Array, index: number): boolean {
  if (index < 0) return false;
  const byte = bytes[index >> 3];
  if (byte === undefined) return false;
  return (byte & (0x80 >> (index & 7))) !== 0;
}

/** Count present pieces in the first `count` positions. */
export function countPieces(bytes: Uint8Array, count: number): number {
  let n = 0;
  for (let i = 0; i < count; i++) if (hasPiece(bytes, i)) n++;
  return n;
}

/**
 * Downsample the bitfield to `buckets` columns, returning the completed
 * fraction (0..1) of each bucket. This is what lets a bar a few hundred pixels
 * wide represent a torrent with hundreds of thousands of pieces: each pixel
 * column summarizes its slice of the bitfield instead of dropping pieces.
 */
export function bucketFractions(
  bytes: Uint8Array,
  totalPieces: number,
  buckets: number,
): Float32Array {
  const out = new Float32Array(Math.max(0, buckets));
  if (totalPieces <= 0 || buckets <= 0) return out;

  for (let b = 0; b < buckets; b++) {
    const start = Math.floor((b * totalPieces) / buckets);
    let end = Math.floor(((b + 1) * totalPieces) / buckets);
    if (end <= start) end = start + 1; // more buckets than pieces: 1 piece each
    let done = 0;
    let seen = 0;
    for (let i = start; i < end && i < totalPieces; i++) {
      if (hasPiece(bytes, i)) done++;
      seen++;
    }
    out[b] = seen === 0 ? 0 : done / seen;
  }
  return out;
}
