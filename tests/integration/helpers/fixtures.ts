// SPDX-License-Identifier: Apache-2.0

/**
 * Fixture registry. All paths are relative to repo root.
 *
 * Factory-generated: `cargo run --example gen_c2pa_fixtures`
 * External (committed): Google Pixel photos with vendor C2PA cert chain
 */

import * as path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "../../..");

export interface FixtureInfo {
  name: string;
  path: string;
  contentType: string;
}

export function fixturePath(relativePath: string): string {
  return path.join(ROOT, relativePath);
}

// ---------------------------------------------------------------------------
// External: Google Pixel (real vendor cert chain, committed)
// ---------------------------------------------------------------------------

export const GOOGLE_SIGNED: FixtureInfo[] = [
  { name: "Pixel plane", path: "tests/fixtures/images/jpeg/pixel_plane.jpg", contentType: "image/jpeg" },
  { name: "Pixel ramen", path: "tests/fixtures/images/jpeg/pixel_ramen.jpg", contentType: "image/jpeg" },
];

// ---------------------------------------------------------------------------
// Factory: C2PA-signed images
// ---------------------------------------------------------------------------

export const C2PA_SIGNED_IMAGES: FixtureInfo[] = [
  { name: "JPEG 4x4", path: "tests/fixtures/c2pa/signed/sample.jpg", contentType: "image/jpeg" },
  { name: "JPEG 640x480", path: "tests/fixtures/c2pa/signed/jpeg-640x480.jpg", contentType: "image/jpeg" },
  { name: "JPEG 1080p", path: "tests/fixtures/c2pa/signed/jpeg-1080p.jpg", contentType: "image/jpeg" },
  { name: "PNG", path: "tests/fixtures/c2pa/signed/sample.png", contentType: "image/png" },
  { name: "TIFF", path: "tests/fixtures/c2pa/signed/sample.tiff", contentType: "image/tiff" },
];

/** All images eligible for PDQ hashing (factory + external). */
export const PDQ_IMAGES: FixtureInfo[] = [
  ...GOOGLE_SIGNED,
  ...C2PA_SIGNED_IMAGES,
];

// ---------------------------------------------------------------------------
// Factory: C2PA-signed video
// ---------------------------------------------------------------------------

export const C2PA_SIGNED_VIDEO: FixtureInfo[] = [
  { name: "MP4 1s 64x64", path: "tests/fixtures/c2pa/signed/mp4-1s-64x64.mp4", contentType: "video/mp4" },
  { name: "MP4 5s 640x480", path: "tests/fixtures/c2pa/signed/mp4-5s-640x480.mp4", contentType: "video/mp4" },
  { name: "MP4 10s 720p", path: "tests/fixtures/c2pa/signed/mp4-10s-720p.mp4", contentType: "video/mp4" },
];

// ---------------------------------------------------------------------------
// Factory: C2PA-signed audio
// ---------------------------------------------------------------------------

export const C2PA_SIGNED_AUDIO: FixtureInfo[] = [
  { name: "WAV 1s", path: "tests/fixtures/c2pa/signed/sample.wav", contentType: "audio/wav" },
  { name: "WAV 5s", path: "tests/fixtures/c2pa/signed/wav-5s.wav", contentType: "audio/wav" },
  { name: "MP3 3s", path: "tests/fixtures/c2pa/signed/sample.mp3", contentType: "audio/mpeg" },
];

// ---------------------------------------------------------------------------
// Factory: Provenance (ingredients)
// ---------------------------------------------------------------------------

export const INGREDIENT_FIXTURES = {
  singleA: { name: "ingredient-a", path: "tests/fixtures/c2pa/signed/ingredient-a.jpg", contentType: "image/jpeg" } as FixtureInfo,
  singleB: { name: "ingredient-b", path: "tests/fixtures/c2pa/signed/ingredient-b.jpg", contentType: "image/jpeg" } as FixtureInfo,
  with2: { name: "with-2-ingredients", path: "tests/fixtures/c2pa/signed/with-2-ingredients.jpg", contentType: "image/jpeg" } as FixtureInfo,
  chain: { name: "with-chain", path: "tests/fixtures/c2pa/signed/with-chain.jpg", contentType: "image/jpeg" } as FixtureInfo,
};

// ---------------------------------------------------------------------------
// All C2PA-signed (for core-c2pa breadth tests)
// ---------------------------------------------------------------------------

export const ALL_C2PA_SIGNED: FixtureInfo[] = [
  ...GOOGLE_SIGNED,
  ...C2PA_SIGNED_IMAGES,
  ...C2PA_SIGNED_VIDEO,
  ...C2PA_SIGNED_AUDIO,
];

// ---------------------------------------------------------------------------
// Unsigned (error-path tests only)
// ---------------------------------------------------------------------------

export const UNSIGNED: FixtureInfo[] = [
  { name: "unsigned JPEG", path: "tests/fixtures/c2pa/unsigned/sample.jpg", contentType: "image/jpeg" },
  { name: "unsigned PNG", path: "tests/fixtures/c2pa/unsigned/sample.png", contentType: "image/png" },
  { name: "unsigned TIFF", path: "tests/fixtures/c2pa/unsigned/sample.tiff", contentType: "image/tiff" },
  { name: "unsigned WAV", path: "tests/fixtures/c2pa/unsigned/sample.wav", contentType: "audio/wav" },
  { name: "unsigned MP4", path: "tests/fixtures/c2pa/unsigned/sample.mp4", contentType: "video/mp4" },
  { name: "unsigned MP3", path: "tests/fixtures/c2pa/unsigned/sample.mp3", contentType: "audio/mpeg" },
  { name: "unsigned JPEG 1080p", path: "tests/fixtures/c2pa/unsigned/jpeg-1080p.jpg", contentType: "image/jpeg" },
  { name: "unsigned MP4 5s", path: "tests/fixtures/c2pa/unsigned/mp4-5s-640x480.mp4", contentType: "video/mp4" },
];
