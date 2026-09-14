import * as esbuild from "esbuild";
import * as fs from "node:fs/promises";
import * as path from "node:path";

const options = {
  entryPoints: ["src/extension.ts"],
  bundle: true,
  outfile: "dist/extension.js",
  external: ["vscode"],
  platform: "node",
  format: "cjs",
  target: "node20",
  sourcemap: true,
  metafile: true,
  legalComments: "linked",
  logLevel: "info",
  plugins: [{
    name: "dependency-notices",
    setup(build) {
      build.onEnd(async (result) => {
        if (!result.metafile) return;
        const packages = new Set();
        for (const input of Object.keys(result.metafile.inputs)) {
          if (!input.includes("node_modules/")) continue;
          let directory = path.dirname(path.resolve(input));
          while (directory !== path.dirname(directory)) {
            if (await fs.access(path.join(directory, "package.json")).then(() => true, () => false)) {
              const manifest = JSON.parse(await fs.readFile(path.join(directory, "package.json"), "utf8"));
              // Some packages have nested module-format manifests with no name.
              if (manifest.name) {
                packages.add(directory);
                break;
              }
            }
            directory = path.dirname(directory);
          }
        }
        const notices = [];
        for (const directory of [...packages].sort()) {
          const manifest = JSON.parse(await fs.readFile(path.join(directory, "package.json"), "utf8"));
          const license = (await fs.readdir(directory)).find((file) => /^licen[sc]e(?:\.(?:md|txt))?$/i.test(file));
          if (!license) throw new Error(`Missing bundled dependency license: ${manifest.name}`);
          notices.push(`${manifest.name} ${manifest.version}\n\n${await fs.readFile(path.join(directory, license), "utf8")}`);
        }
        await fs.writeFile("dist/THIRD_PARTY_NOTICES.txt", notices.join("\n\n---\n\n"));
      });
    },
  }],
};

if (process.argv.includes("--watch")) {
  const context = await esbuild.context(options);
  await context.watch();
} else {
  await esbuild.build(options);
}
