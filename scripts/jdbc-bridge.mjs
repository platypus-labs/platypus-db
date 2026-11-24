// Builds the JDBC bridge (crates/platypus-driver-odbc/java/Bridge.java) into
// crates/platypus-driver-odbc/java/platypus-jdbc-bridge.jar, which the Rust side embeds:
//
//   node scripts/jdbc-bridge.mjs
//
// It needs a JDK 11 or later (`javac` and `jar`, from JAVA_HOME or the PATH). The jar is
// reproducible (fixed timestamps), so a rebuild of an unchanged source changes nothing.
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const java = join(root, "crates", "platypus-driver-odbc", "java");
const exe = (tool) => (process.platform === "win32" ? `${tool}.exe` : tool);
const bin = (tool) => (process.env.JAVA_HOME ? join(process.env.JAVA_HOME, "bin", exe(tool)) : tool);
const scratch = mkdtempSync(join(tmpdir(), "jdbc-bridge-"));
try {
  execFileSync(bin("javac"), ["--release", "11", "-d", scratch, join(java, "Bridge.java")], { stdio: "inherit" });
  execFileSync(
    bin("jar"),
    ["--create", "--file", join(java, "platypus-jdbc-bridge.jar"), "--main-class", "Bridge", "--date", "2026-01-01T00:00:00Z", "-C", scratch, "."],
    { stdio: "inherit" },
  );
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
