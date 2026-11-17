// Raises the app's version everywhere it is written down, and prints the new one.
//
//   node scripts/bump-version.mjs patch|minor|major|x.y.z [--dry-run]
//
// The version lives in five files; they must agree or the installers and the
// updater disagree about what is installed.
import { readFileSync, writeFileSync } from "node:fs";

const [how = "patch", flag] = process.argv.slice(2);
const dryRun = flag === "--dry-run";

const read = (file) => readFileSync(new URL(`../${file}`, import.meta.url), "utf8");
const write = (file, text) => !dryRun && writeFileSync(new URL(`../${file}`, import.meta.url), text);

const current = JSON.parse(read("package.json")).version;
const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(current);
if (!match) throw new Error(`package.json has an unexpected version: ${current}`);
const [major, minor, patch] = match.slice(1).map(Number);

let next;
if (how === "patch") next = `${major}.${minor}.${patch + 1}`;
else if (how === "minor") next = `${major}.${minor + 1}.0`;
else if (how === "major") next = `${major + 1}.0.0`;
else if (/^\d+\.\d+\.\d+$/.test(how)) next = how;
else throw new Error(`Expected patch, minor, major or a version like 1.2.3, not "${how}"`);

const older = (a, b) => {
  const [x, y] = [a, b].map((v) => v.split(".").map(Number));
  return x[0] !== y[0] ? x[0] < y[0] : x[1] !== y[1] ? x[1] < y[1] : x[2] < y[2];
};
if (!older(current, next)) throw new Error(`${next} is not newer than the current ${current}`);

// JSON files: only the version fields change, formatting stays as it is.
for (const file of ["package.json", "package-lock.json", "src-tauri/tauri.conf.json"]) {
  const json = JSON.parse(read(file));
  json.version = next;
  if (json.packages?.[""]) json.packages[""].version = next;
  write(file, JSON.stringify(json, null, 2) + "\n");
}

// Cargo.toml: the version line inside [package].
const cargo = read("src-tauri/Cargo.toml");
const cargoNext = cargo.replace(/(\[package\][^[]*?\nversion = ")[^"]+(")/, `$1${next}$2`);
if (cargoNext === cargo) throw new Error("Could not find the version in src-tauri/Cargo.toml");
write("src-tauri/Cargo.toml", cargoNext);

// Cargo.lock: this app's own entry.
const lock = read("src-tauri/Cargo.lock");
const lockNext = lock.replace(/(name = "dubplate"\nversion = ")[^"]+(")/, `$1${next}$2`);
if (lockNext === lock) throw new Error("Could not find this app in src-tauri/Cargo.lock");
write("src-tauri/Cargo.lock", lockNext);

console.log(next);
