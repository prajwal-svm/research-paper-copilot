// Validates the .research v0 example documents against their JSON Schemas.
// Run: npm run validate:schemas
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const schemaDir = join(root, "schemas", "research-format", "v0");

const load = (p) => JSON.parse(readFileSync(p, "utf8"));

const ajv = new Ajv2020.default({ strict: true, allowUnionTypes: true, allErrors: true });
addFormats.default(ajv);

for (const name of [
  "common",
  "objects",
  "metadata",
  "layout",
  "semantic_tree",
  "citations",
  "knowledge_graph",
]) {
  ajv.addSchema(load(join(schemaDir, `${name}.schema.json`)));
}

const cases = [
  ["metadata", "metadata.json"],
  ["layout", "layout.json"],
  ["semantic_tree", "semantic_tree.json"],
  ["citations", "citations.json"],
  ["knowledge_graph", "knowledge_graph.json"],
];

let failed = false;
for (const [schema, example] of cases) {
  const validate = ajv.getSchema(
    `https://research-paper-copilot.dev/schemas/research-format/v0/${schema}.schema.json`,
  );
  const doc = load(join(schemaDir, "examples", example));
  if (validate(doc)) {
    console.log(`ok   examples/${example}`);
  } else {
    failed = true;
    console.error(`FAIL examples/${example}`);
    for (const err of validate.errors) {
      console.error(`  ${err.instancePath || "/"} ${err.message}`);
    }
  }
}

process.exit(failed ? 1 : 0);
