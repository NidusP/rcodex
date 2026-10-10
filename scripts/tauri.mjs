import { chmodSync, copyFileSync, existsSync, mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";

const args = process.argv.slice(2);
const command = args.find((arg) => !arg.startsWith("-")) ?? "dev";
const isReleaseBuild = command === "build";

if (process.platform !== "darwin") {
  throw new Error("The managed Codex launcher packaging script currently supports macOS only.");
}

const targetTriple = process.arch === "arm64"
  ? "aarch64-apple-darwin"
  : process.arch === "x64"
    ? "x86_64-apple-darwin"
    : null;
if (!targetTriple) throw new Error(`Unsupported macOS architecture: ${process.arch}`);

const profile = isReleaseBuild ? "release" : "debug";
const cargoArgs = ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "codex-launcher", "--bin", "codex-gateway"];
if (isReleaseBuild) cargoArgs.push("--release");

console.log(`Preparing ${profile} Codex sidecar…`);
execFileSync("cargo", cargoArgs, { stdio: "inherit" });

const launcherSource = join("src-tauri", "target", profile, "codex-launcher");
const gatewaySource = join("src-tauri", "target", profile, "codex-gateway");
if (!existsSync(launcherSource) || !existsSync(gatewaySource)) throw new Error(`Sidecar build completed without producing ${launcherSource} and ${gatewaySource}`);
const binariesDir = join("src-tauri", "binaries");
mkdirSync(binariesDir, { recursive: true });
copyFileSync(launcherSource, join(binariesDir, `codex-launcher-${targetTriple}`));
copyFileSync(gatewaySource, join(binariesDir, `codex-gateway-${targetTriple}`));
chmodSync(join(binariesDir, `codex-launcher-${targetTriple}`), 0o755);
chmodSync(join(binariesDir, `codex-gateway-${targetTriple}`), 0o755);

execFileSync(join("node_modules", ".bin", "tauri"), args, { stdio: "inherit" });
