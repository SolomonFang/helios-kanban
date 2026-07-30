const https = require("https");
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

// Replaced during npm pack by workflow (R2 / legacy)
const R2_BASE_URL = "__R2_PUBLIC_URL__";
const BINARY_TAG = "__BINARY_TAG__"; // e.g., v0.0.135-20251215122030
// Replaced in CI: https://github.com/owner/repo/releases/download/v1.2.3
const RELEASE_ASSET_BASE = "__RELEASE_ASSET_BASE__";
const PKG_VERSION = require("../package.json").version;
const CACHE_DIR = path.join(require("os").homedir(), ".vibe-kanban", "bin");

// Local development mode: use binaries from npx-cli/dist/ instead of R2.
// Only activate when running from the actual source repo (has Cargo.toml at root),
// not from an npm-installed package that happens to include a dist/ directory.
const LOCAL_DIST_DIR = path.join(__dirname, "..", "dist");
const LOCAL_DEV_MODE = process.env.VIBE_KANBAN_LOCAL === "1" || (() => {
  const projectRoot = path.join(__dirname, "..", "..");
  return fs.existsSync(path.join(projectRoot, "Cargo.toml"));
})();

async function fetchJson(url) {
  return new Promise((resolve, reject) => {
    https.get(url, (res) => {
      if (res.statusCode === 301 || res.statusCode === 302) {
        return fetchJson(res.headers.location).then(resolve).catch(reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`HTTP ${res.statusCode} fetching ${url}`));
      }
      let data = "";
      res.on("data", (chunk) => (data += chunk));
      res.on("end", () => {
        try {
          resolve(JSON.parse(data));
        } catch (e) {
          reject(new Error(`Failed to parse JSON from ${url}`));
        }
      });
    }).on("error", reject);
  });
}

async function downloadFile(url, destPath, expectedSha256, onProgress) {
  const tempPath = destPath + ".tmp";
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(tempPath);
    const hash = crypto.createHash("sha256");

    const cleanup = () => {
      try {
        fs.unlinkSync(tempPath);
      } catch {}
    };

    https.get(url, (res) => {
      if (res.statusCode === 301 || res.statusCode === 302) {
        file.close();
        cleanup();
        return downloadFile(res.headers.location, destPath, expectedSha256, onProgress)
          .then(resolve)
          .catch(reject);
      }

      if (res.statusCode !== 200) {
        file.close();
        cleanup();
        return reject(new Error(`HTTP ${res.statusCode} downloading ${url}`));
      }

      const totalSize = parseInt(res.headers["content-length"], 10);
      let downloadedSize = 0;

      res.on("data", (chunk) => {
        downloadedSize += chunk.length;
        hash.update(chunk);
        if (onProgress) onProgress(downloadedSize, totalSize);
      });
      res.pipe(file);

      file.on("finish", () => {
        file.close();
        const actualSha256 = hash.digest("hex");
        if (expectedSha256 && actualSha256 !== expectedSha256) {
          cleanup();
          reject(new Error(`Checksum mismatch: expected ${expectedSha256}, got ${actualSha256}`));
        } else {
          try {
            fs.renameSync(tempPath, destPath);
            resolve(destPath);
          } catch (err) {
            cleanup();
            reject(err);
          }
        }
      });
    }).on("error", (err) => {
      file.close();
      cleanup();
      reject(err);
    });
  });
}

function getBundledPlatforms() {
  try {
    return fs
      .readdirSync(LOCAL_DIST_DIR, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name)
      .sort();
  } catch {
    return [];
  }
}

function releaseAssetBaseConfigured() {
  return RELEASE_ASSET_BASE && !RELEASE_ASSET_BASE.startsWith("__");
}

function r2Configured() {
  return !R2_BASE_URL.startsWith("__") && !BINARY_TAG.startsWith("__");
}

function cacheVersionKey() {
  return BINARY_TAG.startsWith("__") ? PKG_VERSION : BINARY_TAG;
}

// Resolve the installed per-platform npm package (esbuild-style
// optionalDependency, e.g. helios-kanban-macos-arm64). Returns the package
// directory (which contains the zips at its root), or null when the package
// is not installed — npm skips optionalDependencies for other platforms.
function getPlatformPackageDir(platform) {
  try {
    return path.dirname(
      require.resolve(`helios-kanban-${platform}/package.json`, {
        paths: [__dirname],
      })
    );
  } catch {
    return null;
  }
}

async function ensureBinary(platform, binaryName, onProgress) {
  // Always prefer packaged local zips when available.
  // Works for both source repo (LOCAL_DEV_MODE) and installed npm package (bundled dist).
  const localZipPath = path.join(LOCAL_DIST_DIR, platform, `${binaryName}.zip`);
  if (fs.existsSync(localZipPath)) {
    return localZipPath;
  }

  const cacheDir = path.join(CACHE_DIR, cacheVersionKey(), platform);
  const zipPath = path.join(cacheDir, `${binaryName}.zip`);
  if (fs.existsSync(zipPath)) return zipPath;
  fs.mkdirSync(cacheDir, { recursive: true });

  // GitHub Release assets (small npm tarball; binaries attached to the same tag)
  if (releaseAssetBaseConfigured()) {
    const base = RELEASE_ASSET_BASE.replace(/\/$/, "");
    const asset = `${platform}-${binaryName}.zip`;
    const url = `${base}/${encodeURIComponent(asset)}`;
    await downloadFile(url, zipPath, null, onProgress);
    return zipPath;
  }

  if (r2Configured()) {
    const manifest = await fetchJson(`${R2_BASE_URL}/binaries/${BINARY_TAG}/manifest.json`);
    const binaryInfo = manifest.platforms?.[platform]?.[binaryName];

    if (!binaryInfo) {
      throw new Error(`Binary ${binaryName} not available for ${platform}`);
    }

    const url = `${R2_BASE_URL}/binaries/${BINARY_TAG}/${platform}/${binaryName}.zip`;
    await downloadFile(url, zipPath, binaryInfo.sha256, onProgress);
    return zipPath;
  }

  const bundled = getBundledPlatforms();
  const list = bundled.length ? bundled.join(", ") : "none";
  throw new Error(
    `No bundled binary for ${platform}. Bundled platforms: ${list}. ` +
      "Remote download is not configured in this package."
  );
}

async function getLatestVersion() {
  const manifest = await fetchJson(`${R2_BASE_URL}/binaries/manifest.json`);
  return manifest.latest;
}

module.exports = {
  R2_BASE_URL,
  BINARY_TAG,
  RELEASE_ASSET_BASE,
  CACHE_DIR,
  LOCAL_DEV_MODE,
  LOCAL_DIST_DIR,
  ensureBinary,
  getLatestVersion,
  getPlatformPackageDir,
};
