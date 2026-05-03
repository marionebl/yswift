#!/usr/bin/env node
// Regenerates the binary update fixture used by the wire-format test in
// `lib/tests/preperoni_wire_format.rs`. Run with:
//
//   cd lib/tests/fixtures
//   npm install --no-save yjs@13
//   node generate_preperoni_fixture.mjs
//
// The fixture encodes the preperoni schema described in
// `docs/briefs/ios-yswift-handover.md` §2:
//
//   recipes: Y.Map<string, Y.Map<string, Scalar>>
//   plans:   Y.Map<string, Y.Map<string, Y.Array<string> | Scalar>>
//
// The Rust side asserts it can `apply_update` this blob and recover the same
// nested structure through the new `YrsValueKind` / typed getters.
//
// We commit the resulting `preperoni_v1.bin` so the Rust test runs without
// a node toolchain in CI; this script exists so future schema changes can
// regenerate the fixture deterministically.

import * as Y from 'yjs';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));

const doc = new Y.Doc();
const recipes = doc.getMap('recipes');
const plans = doc.getMap('plans');

// ---- recipes ----
const rec = new Y.Map();
rec.set('url', 'https://example.com/pasta');
rec.set('status', 'parsed');
rec.set('title', 'Pasta');
rec.set('image_url', null);
rec.set('servings', 4);
rec.set('active_time_min', 15);
rec.set('total_time_min', 25);
rec.set('created_at', '2025-04-30T10:00:00.000Z');
rec.set('parsed_at', '2025-04-30T10:00:01.000Z');
rec.set('liked_at', null);
recipes.set('rec_abc', rec);

// ---- plans ----
const plan = new Y.Map();
const recipeIds = new Y.Array();
recipeIds.insert(0, ['rec_abc']);
plan.set('recipeIds', recipeIds);
const lockedUrls = new Y.Array();
lockedUrls.insert(0, ['https://example.com/pasta']);
plan.set('lockedUrls', lockedUrls);
plan.set('groceryJson', '{"items":[]}');
plan.set('createdAt', '2025-04-30T10:00:02.000Z');
plan.set('rerollCount', 0);
plans.set('2025-18', plan);

const update = Y.encodeStateAsUpdate(doc);
writeFileSync(join(here, 'preperoni_v1.bin'), update);
console.log(
    `wrote ${update.byteLength} bytes to preperoni_v1.bin`,
);
