import { chmodSync, copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";

const args = process.argv.slice(2).filter((arg) => arg !== "--");
const command = args.find((arg) => !arg.startsWith("-")) ?? "dev";
const isReleaseBuild = command === "build";

function optionValue(argv, name) {
  const inline = argv.find((arg) => arg.startsWith(`${name}=`));
  if (inline) return inline.slice(name.length + 1);
  const index = argv.indexOf(name);
  if (index !== -1 && argv[index + 1] && !argv[index + 1].startsWith("-")) return argv[index + 1];
  return null;
}

function hostTriple() {
  if (process.platform === "darwin" && process.arch === "arm64") return "aarch64-apple-darwin";
  if (process.platform === "darwin" && process.arch === "x64") return "x86_64-apple-darwin";
  if (process.platform === "win32" && process.arch === "x64") return "x86_64-pc-windows-msvc";
  if (process.platform === "win32" && process.arch === "arm64") return "aarch64-pc-windows-msvc";
  throw new Error(`Unsupported packaging platform: ${process.platform} ${process.arch}`);
}

const requestedTarget = optionValue(args, "--target");
const targetTriple = requestedTarget ?? hostTriple();
const executableSuffix = targetTriple.includes("windows") ? ".exe" : "";
const profile = isReleaseBuild ? "release" : "debug";
const artifactDir = requestedTarget
  ? join("src-tauri", "target", requestedTarget, profile)
  : join("src-tauri", "target", profile);

const cargoArgs = ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "codex-launcher", "--bin", "codex-gateway"];
if (requestedTarget) cargoArgs.push("--target", requestedTarget);
if (isReleaseBuild) cargoArgs.push("--release");

const binariesDir = join("src-tauri", "binaries");
mkdirSync(binariesDir, { recursive: true });
for (const name of ["codex-launcher", "codex-gateway"]) {
  const sidecarPath = join(binariesDir, `${name}-${targetTriple}${executableSuffix}`);
  if (!existsSync(sidecarPath)) {
    // build.rs checks these paths while compiling the sidecar package, before this script can copy the real binaries.
    writeFileSync(sidecarPath, "");
    chmodSync(sidecarPath, 0o755);
  }
}

console.log(`Preparing ${profile} sidecar for ${targetTriple}…`);
execFileSync("cargo", cargoArgs, { stdio: "inherit" });

for (const name of ["codex-launcher", "codex-gateway"]) {
  const source = join(artifactDir, `${name}${executableSuffix}`);
  const destination = join(binariesDir, `${name}-${targetTriple}${executableSuffix}`);
  if (!existsSync(source)) throw new Error(`Sidecar build completed without producing ${source}`);
  copyFileSync(source, destination);
  chmodSync(destination, 0o755);
}

const tauriBin = process.platform === "win32"
  ? join("node_modules", ".bin", "tauri.cmd")
  : join("node_modules", ".bin", "tauri");
const tauriArgs = [...args];
const tauriEnv = { ...process.env };
if (command === "dev") {
  tauriEnv.RCODEX_GATEWAY_PORT = "43124";
  if (!optionValue(tauriArgs, "--config")) {
    tauriArgs.push("--config", "src-tauri/tauri.dev.conf.json");
  }
}
if (isReleaseBuild) {
  // A live `tauri dev` leaves TAURI_CONFIG in the environment. If a production
  // build inherits it, the binary ships with io.rcodex.dev and wakes the Dev app.
  delete tauriEnv.TAURI_CONFIG;
  delete tauriEnv.RCODEX_GATEWAY_PORT;
  if (process.platform === "win32" && !optionValue(tauriArgs, "--bundles")) {
    tauriArgs.push("--bundles", "nsis");
  }
}
execFileSync(tauriBin, tauriArgs, {
  stdio: "inherit",
  shell: process.platform === "win32",
  env: tauriEnv,
});
