/**
 * Prints one of the mock backend's demo input texts to stdout (`demo`, `harsh` or `empty`).
 *
 * `tools/gen-ui-fixtures.sh` pipes the text through `scrub --json` to produce
 * `src/lib/mock/fixtures/scan-<name>.json`, so the fixtures are the real engine's output. The
 * fake secrets are assembled at runtime in `src/lib/mock/demo.ts`, never written out as
 * contiguous literals, which keeps secret scanners quiet. Run with plain Node (it strips the
 * types): `node scripts/demo-fixture.ts demo`.
 */
import { DEMO_INPUTS } from '../src/lib/mock/demo.ts';

const name = process.argv[2] ?? '';
const input = DEMO_INPUTS[name];
if (!input) {
  console.error(`usage: node scripts/demo-fixture.ts <${Object.keys(DEMO_INPUTS).join('|')}>`);
  process.exit(2);
}
process.stdout.write(input());
