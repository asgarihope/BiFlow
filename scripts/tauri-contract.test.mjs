import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/**
 * Strip whole-line `;` comments from an NSIS script.
 *
 * The installer hook documents *why* it avoids a construct (a LogicLib
 * conditional, `$PROGRAMDATA`). Scanning the raw text would then either fail
 * the negative contract on the explanation, or let the explanation satisfy a
 * positive one. The rule is the same as the Rust source contracts below: read
 * the production lines only.
 */
function nsisCode(source) {
  return source
    .split("\n")
    .filter((line) => !line.trimStart().startsWith(";"))
    .join("\n");
}

/**
 * Strip whole-line `#` comments from a PowerShell script.
 *
 * The build scripts explain *why* they avoid a command, and a raw scan then
 * matches that explanation instead of the command. Same rule as nsisCode.
 */
function scriptCode(source) {
  return source
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("#"))
    .join("\n");
}

function rustSources(directory) {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (
      entry.name === "target" ||
      entry.name === "node_modules" ||
      entry.name === ".git"
    ) {
      continue;
    }
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...rustSources(path));
    } else if (entry.name.endsWith(".rs")) {
      files.push(path);
    }
  }
  return files;
}

function requiredPlatformBackendMethods(traitSource) {
  return traitSource
    .split("async fn ")
    .slice(1)
    .filter((chunk) => {
      const semicolon = chunk.indexOf(";");
      const brace = chunk.indexOf("{");
      return semicolon !== -1 && (brace === -1 || semicolon < brace);
    })
    .map((chunk) => /^(\w+)/.exec(chunk)[1]);
}

describe("Tauri frontend contract", () => {
  it("registers every Rust command invoked by the production UI", () => {
    const frontend = readFileSync(
      join(root, "apps/desktop/src/api/desktop.ts"),
      "utf8",
    );
    const rust = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    const invoked = [...frontend.matchAll(/\binvoke\("([a-z0-9_]+)"/g)].map(
      (match) => match[1],
    );
    const handlerBlock = rust.match(/tauri::generate_handler!\[([\s\S]*?)\]\)/);

    assert.ok(handlerBlock, "Rust invoke handler registration is missing");
    const registered = handlerBlock[1]
      .split(",")
      .map((command) => command.trim())
      .filter(Boolean);
    const missing = [...new Set(invoked)].filter(
      (command) => !registered.includes(command),
    );

    assert.deepEqual(missing, []);
    assert.match(frontend, /window\.__TAURI_INTERNALS__ !== undefined/);
    assert.match(frontend, /listen<StackSnapshot>\("stack-snapshot"/);
    assert.match(rust, /emit\("stack-snapshot"/);
    assert.match(rust, /emit\("update-progress"/);
  });

  it("picks side-tunnel profiles through the Tauri dialog plugin", () => {
    const rust = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    const cargo = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
    const frontend = readFileSync(
      join(root, "apps/desktop/src/api/desktop.ts"),
      "utf8",
    );
    const picker = readFileSync(
      join(root, "src-tauri/src/profile_picker.rs"),
      "utf8",
    );
    assert.match(cargo, /tauri-plugin-dialog/);
    assert.match(rust, /tauri_plugin_dialog::init\(\)/);
    assert.match(rust, /fn pick_client_profile/);
    assert.match(frontend, /invoke\("pick_client_profile"\)/);
    assert.match(
      picker,
      /add_filter\("VPN profile", accepted_profile_extensions\(\)\)/,
    );
    assert.doesNotMatch(picker, /info!\(\s*path\b/);
    assert.doesNotMatch(picker, /blocking_pick_file/);
    assert.match(picker, /\.pick_file\(/);
    assert.match(rust, /async fn pick_client_profile/);
    assert.match(frontend, /invoke\("apply_live_settings"\)/);
    assert.match(rust, /async fn apply_live_settings/);
  });

  it("reads the native clipboard through the Tauri plugin", () => {
    const rust = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    const cargo = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
    const capabilities = JSON.parse(
      readFileSync(join(root, "src-tauri/capabilities/main.json"), "utf8"),
    );
    assert.match(cargo, /tauri-plugin-clipboard-manager/);
    assert.match(rust, /tauri_plugin_clipboard_manager::init\(\)/);
    assert.ok(
      capabilities.permissions.includes("clipboard-manager:allow-read-text"),
    );
    assert.ok(
      capabilities.permissions.includes("clipboard-manager:allow-write-text"),
    );
  });

  it("points NSIS helper staging at ProgramData iran-split staging", () => {
    const hook = nsisCode(
      readFileSync(join(root, "packaging/windows/installer-hooks.nsh"), "utf8"),
    );
    const desktop = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    const install = readFileSync(
      join(root, "src-tauri/src/helper_install.rs"),
      "utf8",
    );
    const helper = readFileSync(
      join(root, "crates/iran-split-helper/src/windows.rs"),
      "utf8",
    );
    // NSIS has no `$PROGRAMDATA`. An unknown `$...` token expands to empty, so
    // the old `$PROGRAMDATA\iran-split\staging` silently dropped the helper's
    // required --staging-dir value and the install aborted at the very end. The
    // negative assertion is the real guard: `$COMMONPROGRAMDATA` cannot match
    // `\$PROGRAMDATA`, so reintroducing the typo fails here instead of silently.
    assert.match(hook, /\$COMMONPROGRAMDATA\\iran-split\\staging/);
    assert.doesNotMatch(hook, /\$PROGRAMDATA/);
    assert.doesNotMatch(hook, /\$LOCALAPPDATA\\biflow\\runtime\\generations/);
    assert.ok(install.includes(String.raw`C:\ProgramData\iran-split\staging`));
    assert.match(desktop, /generation_staging_dir/);
    assert.match(helper, /root\.join\("staging"\)/);
    assert.match(helper, /fn grant_users_modify/);
    const windows = readFileSync(
      join(root, "crates/iran-split-platform-win/src/lib.rs"),
      "utf8",
    );
    // `#![cfg(windows)]` hides this crate from Linux `cargo test`. Scan the
    // production half so a source-contract `!contains` cannot match itself.
    const production = windows.split("mod tests {")[0];
    assert.match(production, /\.generation_staging_dir/);
    assert.doesNotMatch(
      production,
      /user_data_dir\.join\("runtime"\)\.join\("generations"\)/,
    );
  });

  it("keeps a resizable main window with a 390x640 minimum", () => {
    const config = JSON.parse(
      readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
    );
    const window = config.app.windows[0];
    assert.equal(window.width, 1120);
    assert.equal(window.height, 760);
    assert.equal(window.minWidth, 390);
    assert.equal(window.minHeight, 640);
    assert.equal(window.maxWidth, undefined);
    assert.equal(window.maxHeight, undefined);
    assert.equal(window.resizable, true);
    const rust = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    assert.match(rust, /restore_main_window_size/);
    assert.match(rust, /persist_main_window_size/);
  });

  it("uses the static application logo for the tray icon", () => {
    const rust = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    // The tray is a standard Windows application icon: the bundled
    // window icon, a plain title, and nothing generated at runtime. A
    // regression that re-introduces a painted bitmap (live rates,
    // status text, a synthesised image) would smuggle data back into
    // the shell icon, which is exactly what the Windows 11 Widgets
    // Board now exists to handle instead.
    assert.match(rust, /app\s*\n?\s*\.default_window_icon\(\)/);
    assert.match(
      rust,
      /TrayIconBuilder::with_id\("main"\)[\s\S]*?\.icon\(logo\)[\s\S]*?\.menu\(&menu\)/,
    );
    assert.match(rust, /\.tooltip\("BiFlow"\)/);
    // No renderer, no rate sampler, no live badge in the shell.
    assert.doesNotMatch(rust, /initial_tray_icon/);
    assert.doesNotMatch(rust, /render_rate_icon/);
    assert.doesNotMatch(rust, /set_icon\(/);
    assert.match(rust, /MenuItem::with_id\(\s*app,\s*"dashboard"/);
    assert.match(rust, /app.emit\("app-navigate", "dashboard"\)/);
    assert.match(rust, /apply_tray_menu/);
    const tray = readFileSync(join(root, "src-tauri/src/tray.rs"), "utf8");
    assert.match(tray, /fn labels_for/);
    assert.match(tray, /fn actions_enabled/);
    assert.match(rust, /reserve_lifecycle/);
  });

  it("bundles and registers the Windows 11 widget package from the NSIS installer", () => {
    const conf = JSON.parse(
      readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
    );
    const nsis = conf.bundle.windows.nsis;

    // The MSIX is staged into the installer through `bundle.resources`;
    // Tauri v2's NsisConfig has no `installerFiles` key, so a
    // well-meaning "just add the file" edit lands in schema rejection.
    assert.equal(
      nsis.installerFiles,
      undefined,
      "NsisConfig has no installerFiles; extra files belong in bundle.resources",
    );
    assert.match(
      conf.bundle.resources["../packaging/staged/windows/widget/"],
      /^widget\//,
    );

    const hooks = nsisCode(
      readFileSync(join(root, "packaging/windows/installer-hooks.nsh"), "utf8"),
    );
    assert.match(hooks, /!macro NSIS_HOOK_POSTINSTALL/);
    assert.match(
      hooks,
      /IfFileExists "\$INSTDIR\\widget\\BiFlow\.WidgetProvider\.msix"/,
    );
    assert.match(hooks, /Add-AppxPackage/);
    assert.match(hooks, /!macro NSIS_HOOK_PREUNINSTALL/);
    assert.match(hooks, /Remove-AppxPackage/);

    // The widget is optional: its failure must never abort the install.
    // Only the Helper is allowed to `Abort`, so count the aborts and pin
    // that exactly one remains, on the Helper path.
    const postinstall = hooks.match(
      /!macro NSIS_HOOK_POSTINSTALL[\s\S]*?!macroend/,
    )[0];
    assert.equal(
      (postinstall.match(/^\s*Abort /gm) || []).length,
      1,
      "only the Helper failure may abort the install",
    );
    assert.match(postinstall, /Abort "BiFlow Helper installation failed"/);
    assert.doesNotMatch(postinstall, /Abort[^"]*widget/i);

    // The Tauri NSIS template does not include LogicLib, so the hooks must
    // use plain jumps. A stray `${If}` compiles on the dev machine only if
    // someone added the include, and fails in CI otherwise.
    assert.doesNotMatch(hooks, /\$\{(If|EndIf|StrCmp)\b/);

    // Windows 10 must stay a no-op: the package itself is what the OS
    // decides to ignore, and the desktop never depends on it.
    const manifest = readFileSync(
      join(root, "widgets/WindowsWidgetProvider/AppxManifest.xml"),
      "utf8",
    );
    // `windows.widgets` is NOT a category the packaging schema accepts. The
    // registration is a uap3 app extension named com.microsoft.windows.widgets
    // whose free-form uap3:Properties payload the widget host reads. Both
    // spellings below were rejected by makeappx (C00CE014) in turn.
    assert.match(manifest, /Category="windows\.comServer"/);
    assert.match(manifest, /uap3:Extension Category="windows\.appExtension"/);
    assert.match(manifest, /Name="com\.microsoft\.windows\.widgets"/);
    assert.match(manifest, /MinVersion="10\.0\.17763\.0"/);
    assert.doesNotMatch(manifest, /uap3:AppExtension Category=/);

    // Screenshots is REQUIRED inside ThemeResources, and it is the element the
    // widget picker renders. Omit it and the package installs, the COM class
    // registers, and the widget still never appears - the host has nothing to
    // show in the Add Widgets dialog and drops the provider silently.
    assert.match(manifest, /<Screenshots>/);
    assert.match(manifest, /Path="Assets\\WidgetScreenshot\.png"/);
    assert.match(manifest, /DisplayAltText="/);
    // The picker expects 300x304 with transparent rounded corners.
    const screenshot = readFileSync(
      join(root, "widgets/WindowsWidgetProvider/Assets/WidgetScreenshot.png"),
    );
    assert.equal(
      screenshot.readUInt32BE(16),
      300,
      "screenshot width must be 300",
    );
    assert.equal(
      screenshot.readUInt32BE(20),
      304,
      "screenshot height must be 304",
    );

    // The CLSID is duplicated in three places: the com:Class registration,
    // the widget host's CreateInstance activation, and ProviderClsid in the
    // C++ provider. A mismatch is a completely silent no-op - the widget host
    // just never calls anything - so assert the three agree.
    const ids = readFileSync(
      join(root, "widgets/WindowsWidgetProvider/src/WidgetIds.h"),
      "utf8",
    );
    const manifestClsids = [
      ...manifest.matchAll(/7B3C1A62-4F5D-4E2B-9C8A-1D6E5A9F3B47/g),
    ];
    assert.equal(
      manifestClsids.length,
      2,
      "the manifest must carry the CLSID on com:Class and on CreateInstance",
    );
    assert.match(ids, /ProviderClsid/);
    assert.match(
      ids,
      /0x7B3C1A62, 0x4F5D, 0x4E2B, \{0x9C, 0x8A, 0x1D, 0x6E, 0x5A, 0x9F, 0x3B, 0x47\}/,
      "ProviderClsid must match the manifest CLSID exactly",
    );

    // The provider is C++/WinRT: the class factory and the IWidgetProvider
    // vtable are generated by winrt::implements. A return to C#/WinRT would
    // mean hand-written COM thunks, because ComExport no longer exists in
    // .NET and the Windows App SDK does not project CWidgetProvider.
    const provider = readFileSync(
      join(root, "widgets/WindowsWidgetProvider/src/WidgetProviderMain.cpp"),
      "utf8",
    );
    assert.match(provider, /winrt::implements<Provider, wp::IWidgetProvider>/);
    assert.match(
      provider,
      /winrt::implements<ProviderFactory, ::IClassFactory/,
    );
    assert.match(provider, /CoRegisterClassObject/);
    // Every member of the real IWidgetProvider. An invented surface compiles
    // as an unimplemented class and then silently never gets called.
    for (const method of [
      "CreateWidget",
      "DeleteWidget",
      "Activate",
      "Deactivate",
      "OnActionInvoked",
      "OnWidgetContextChanged",
    ]) {
      assert.match(
        provider,
        new RegExp(`void ${method}\\(`),
        `missing ${method}`,
      );
    }
    // <unknwn.h> must precede the C++/WinRT headers or implements<> refuses
    // to build a vtable for a classic COM interface.
    assert.ok(
      provider.indexOf("#include <unknwn.h>") <
        provider.indexOf("#include <winrt/base.h>"),
      "unknwn.h must be included before any C++/WinRT header",
    );

    // A widget provider must be installed from the packed MSIX. A loose
    // Add-AppxPackage -Register deployment leaves an AppModel repository entry
    // with no subkeys, so the app extension is never indexed and the Widgets
    // Board silently never lists the provider. The build must therefore
    // delegate to the self-elevating installer and must never use -Register.
    const build = scriptCode(
      readFileSync(
        join(root, "widgets/WindowsWidgetProvider/build.ps1"),
        "utf8",
      ),
    );
    assert.match(build, /install-widget\.ps1/);
    assert.doesNotMatch(
      build,
      /Add-AppxPackage\s+-Register/,
      "a loose -Register deployment never indexes the widget app extension",
    );
    const installer = readFileSync(
      join(root, "widgets/WindowsWidgetProvider/install-widget.ps1"),
      "utf8",
    );
    // The trust step is what needs elevation: the package signature chain must
    // terminate in Cert:\LocalMachine\Root.
    assert.match(installer, /certutil\.exe -addstore -f Root/);
    assert.match(installer, /Add-AppxPackage -Path \$msix/);
    assert.match(installer, /-Verb RunAs/);
  });

  it("disables WebKitGTK DMA-BUF rendering before the Linux webview starts", () => {
    const rust = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    assert.match(rust, /WEBKIT_DISABLE_DMABUF_RENDERER/);
    assert.match(rust, /WEBKIT_DISABLE_COMPOSITING_MODE/);
    assert.match(rust, /LIBGL_ALWAYS_SOFTWARE/);
    assert.match(rust, /apply_linux_webview_workarounds\(\)/);
    assert.match(rust, /BIFLOW_WEBKIT_WORKAROUNDS/);
    const applyFn = rust.match(
      /fn apply_linux_webview_workarounds\(\) \{[\s\S]*?\n\}/,
    );
    assert.ok(applyFn, "apply_linux_webview_workarounds is missing");
    assert.match(applyFn[0], /command\.exec\(\)/);
    assert.doesNotMatch(applyFn[0], /\.status\(\)/);
    assert.match(rust, /current_exe\(\)/);
    assert.match(rust, /single_instance_dbus_id/);
    assert.match(rust, /dbus_id\(single_instance_dbus_id/);
    const run = rust.match(/pub fn run\(\) \{([\s\S]*?)let builder =/);
    assert.ok(run, "run() is missing");
    const apply = run[1].indexOf("apply_linux_webview_workarounds");
    const diagnostics = run[1].indexOf("initialize_diagnostics");
    assert.ok(
      apply >= 0 && diagnostics > apply,
      "WebKit env must be set before diagnostics and GTK start",
    );
  });

  it("does not attach a console or leftover terminal when the GUI starts", () => {
    const desktopMain = readFileSync(
      join(root, "src-tauri/src/main.rs"),
      "utf8",
    );
    const helperMain = readFileSync(
      join(root, "crates/iran-split-helper/src/main.rs"),
      "utf8",
    );
    const config = JSON.parse(
      readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
    );
    const desktop = readFileSync(
      join(root, "packaging/linux/app.desktop"),
      "utf8",
    );
    assert.match(
      desktopMain,
      /#!\[cfg_attr\(windows, windows_subsystem = "windows"\)\]/,
    );
    assert.match(
      helperMain,
      /#!\[cfg_attr\(windows, windows_subsystem = "windows"\)\]/,
    );
    assert.equal(
      config.bundle.linux.deb.desktopTemplate,
      "../packaging/linux/app.desktop",
    );
    assert.match(desktop, /^Terminal=false$/m);
  });

  it("commits the current updater public key and verifies signing via the Tauri CLI", () => {
    const config = JSON.parse(
      readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
    );
    const prepare = readFileSync(
      join(root, "scripts/prepare-tauri-signing.mjs"),
      "utf8",
    );
    const decoded = Buffer.from(
      config.plugins.updater.pubkey,
      "base64",
    ).toString("utf8");
    assert.match(
      decoded,
      /untrusted comment: minisign public key: 4D42AC0D9C21345C/,
    );
    assert.match(prepare, /node_modules\/@tauri-apps\/cli\/tauri\.js/);
    assert.doesNotMatch(prepare, /spawn\("pnpm"/);
  });

  it("passes Win32 security-descriptor out-params as raw pointers", () => {
    const source = readFileSync(
      join(root, "crates/iran-split-helper-winacl/src/windows_impl.rs"),
      "utf8",
    );
    assert.match(source, /&raw mut descriptor/);
    assert.doesNotMatch(
      source,
      /ConvertStringSecurityDescriptorToSecurityDescriptorW\([\s\S]*?&mut descriptor/,
    );
  });

  it("keeps Unix-only OpenOptions inside the Unix sync_directory function", () => {
    const config = readFileSync(
      join(root, "crates/iran-split-config/src/lib.rs"),
      "utf8",
    );
    assert.doesNotMatch(config, /use std::fs::OpenOptions/);
    assert.doesNotMatch(config, /fs::\{self, OpenOptions\}/);
    assert.match(config, /fs::OpenOptions::new\(\)/);
  });

  it("keeps both platform backends staging the same runtime generation", () => {
    const linux = readFileSync(
      join(root, "crates/iran-split-platform-linux/src/lib.rs"),
      "utf8",
    );
    const windows = readFileSync(
      join(root, "crates/iran-split-platform-win/src/lib.rs"),
      "utf8",
    );
    // The staging helpers are duplicated per platform (ADR 0033). A rule file
    // added on one side and not the other ships a config Mihomo cannot load.
    const providers = [
      "private.txt",
      "iran-domains.txt",
      "iran-networks.txt",
      "iran-business-domains.txt",
      "iran-cdn-networks.txt",
      "custom-direct-domains.txt",
      "custom-direct-ips.txt",
      "config.yaml",
    ];
    for (const name of providers) {
      assert.ok(linux.includes(name), `${name} missing from the Linux backend`);
      assert.ok(
        windows.includes(name),
        `${name} missing from the Windows backend`,
      );
    }
    assert.ok(
      linux.includes("provider_files"),
      "Linux backend must emit per-client provider files",
    );
    assert.ok(
      windows.includes("provider_files"),
      "Windows backend must emit per-client provider files",
    );
    // Each backend must pin its own Platform value or the generated config
    // silently loses the Windows-only strict-route flag.
    assert.match(
      linux,
      /generate_config_with_handles\(\s*&config,\s*Platform::Linux,/,
    );
    assert.match(
      windows,
      /generate_config_with_handles\(\s*&config,\s*Platform::Windows,/,
    );
    // A readiness timeout used to be CoreError::Platform, which the UI maps
    // to "An internal error occurred." Keep both backends on the typed error.
    assert.match(linux, /fn readiness_error/);
    assert.match(windows, /fn readiness_error/);
    assert.match(linux, /CoreError::ControllerTimeout/);
    assert.match(windows, /CoreError::ControllerTimeout/);
    assert.match(linux, /CoreError::ControllerUnauthorized/);
    assert.match(windows, /CoreError::ControllerUnauthorized/);
    // Both compile only for their own OS, so host Clippy never sees the other.
    assert.match(linux, /^#!\[cfg\(target_os = "linux"\)\]/m);
    assert.match(windows, /^#!\[cfg\(windows\)\]/m);
  });

  it("implements every PlatformBackend method on Windows", () => {
    const core = readFileSync(
      join(root, "crates/iran-split-core/src/lib.rs"),
      "utf8",
    );
    const windows = readFileSync(
      join(root, "crates/iran-split-platform-win/src/lib.rs"),
      "utf8",
    );
    const trait = core.match(/pub trait PlatformBackend[\s\S]*?\n\}/);
    assert.ok(trait, "PlatformBackend trait is missing");
    // A signature ending in `;` has no default body, so every implementor must
    // provide it. Methods with a default (`stop_user_proxy`,
    // `verify_not_intercepting`) are checked separately.
    const required = requiredPlatformBackendMethods(trait[0]);
    assert.ok(required.length > 8, "trait parse found too few methods");

    const impl = windows.match(
      /impl PlatformBackend for WindowsBackend \{[\s\S]*?\r?\n\}\r?\n/,
    );
    assert.ok(impl, "WindowsBackend does not implement PlatformBackend");
    const missing = required.filter(
      (name) => !new RegExp(`async fn ${name}\\(`).test(impl[0]),
    );
    assert.deepEqual(missing, [], "Windows backend is missing trait methods");
    // Defaulted in the trait, but a backend that never stops the user's proxy
    // leaves Hiddify running after Disconnect.
    assert.match(impl[0], /async fn stop_user_proxy\(/);
    assert.match(impl[0], /async fn clear_hiddify_system_proxy\(/);
    assert.match(impl[0], /async fn restore_hiddify_system_proxy\(/);
    // The pre-2.2.0 stub answered every call with one canned platform error.
    assert.doesNotMatch(windows, /fn unavailable<T>\(\)/);
  });

  it("implements every required PlatformBackend method on the CLI demo", () => {
    const core = readFileSync(
      join(root, "crates/iran-split-core/src/lib.rs"),
      "utf8",
    );
    const cli = readFileSync(
      join(root, "crates/iran-split-cli/src/main.rs"),
      "utf8",
    );
    const trait = core.match(/pub trait PlatformBackend[\s\S]*?\n\}/);
    assert.ok(trait, "PlatformBackend trait is missing");
    const required = requiredPlatformBackendMethods(trait[0]);
    const impl = cli.match(
      /impl PlatformBackend for DemoBackend \{[\s\S]*?\r?\n\}\r?\n/,
    );
    assert.ok(impl, "DemoBackend does not implement PlatformBackend");
    const missing = required.filter(
      (name) => !new RegExp(`async fn ${name}\\(`).test(impl[0]),
    );
    assert.deepEqual(missing, [], "CLI DemoBackend is missing trait methods");
  });

  it("checks GitHub Releases from About and never polls in the background", () => {
    const rust = readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8");
    const githubUpdate = readFileSync(
      join(root, "src-tauri/src/github_update.rs"),
      "utf8",
    );

    assert.match(
      githubUpdate,
      /https:\/\/api\.github\.com\/repos\/devlifeX\/BiFlow\/releases\/latest/,
    );
    assert.match(
      rust,
      /async fn check_for_update\([\s\S]*?collect_update_status\(&app, "tauri_command"\)/,
    );
    assert.match(
      rust,
      /async fn collect_update_status\([\s\S]*?fetch_github_update\(app, initiator\)/,
    );
    assert.match(rust, /fn fetch_github_update\(/);
    assert.match(rust, /async fn fetch_verified_update_bytes\(/);
    assert.match(rust, /signed\.download\(/);
    assert.match(rust, /validate_signed_metadata\(/);
    assert.doesNotMatch(githubUpdate, /async fn download_asset\(/);
    assert.match(githubUpdate, /pkexec/);
    assert.match(githubUpdate, /apt-get/);
    assert.match(githubUpdate, /apply-update\.ps1/);
    assert.match(githubUpdate, /-Verb RunAs -Wait -PassThru/);
    assert.match(githubUpdate, /\$process\.ExitCode -ne 0/);
    assert.match(githubUpdate, /1223/);
    assert.match(rust, /\.try_begin\(/);
    assert.match(rust, /phase: "installed"/);
    assert.doesNotMatch(rust, /fn spawn_background_update_checks\(/);
    assert.doesNotMatch(rust, /fn fetch_signed_update\(/);
    assert.doesNotMatch(rust, /open_linux_deb_release/);
    assert.doesNotMatch(rust, /an update is already in progress/);
    assert.doesNotMatch(githubUpdate, /trace_route = .*browser_download_url/);
  });

  it("never reports a previous attempt's Windows install reason", () => {
    const source = readFileSync(
      join(root, "src-tauri/src/helper_install.rs"),
      "utf8",
    );
    // The elevated helper writes install.log only when it reaches its own
    // error path, so a stale file must be cleared before each elevation.
    const install = source.match(
      /async fn install_windows\([\s\S]*?\r?\n\}\r?\n/,
    );
    assert.ok(install, "install_windows is missing");
    const cleared = install[0].indexOf("discard_stale_install_log()");
    const elevated = install[0].indexOf('Command::new("powershell")');
    assert.ok(cleared >= 0, "the stale install log is never cleared");
    assert.ok(
      elevated > cleared,
      "install.log must be cleared before the elevation runs",
    );
    assert.match(source, /fn discard_stale_install_log\(/);
  });

  it("fails NSIS install and uninstall when the privileged Helper command fails", () => {
    const hooks = nsisCode(
      readFileSync(join(root, "packaging/windows/installer-hooks.nsh"), "utf8"),
    );
    for (const [macro, command] of [
      ["POSTINSTALL", "--install"],
      ["PREUNINSTALL", "--uninstall"],
    ]) {
      const body = hooks.match(
        new RegExp(`!macro NSIS_HOOK_${macro}([\\s\\S]*?)!macroend`),
      )?.[1];
      assert.ok(body, `${macro} hook is missing`);
      assert.match(body, new RegExp(command));
      assert.match(body, /nsExec::ExecToLog[\s\S]*?Pop \$R0/);
      assert.match(body, /StrCmp \$R0 "0"/);
      assert.match(body, /SetErrorLevel 1[\s\S]*?Abort/);
    }
  });

  it("preserves and restarts the Linux Helper on package upgrades", () => {
    const postinst = readFileSync(
      join(root, "packaging/linux/postinst.sh"),
      "utf8",
    );
    const prerm = readFileSync(join(root, "packaging/linux/prerm.sh"), "utf8");
    const helperInstaller = readFileSync(
      join(root, "packaging/linux/install-helper.sh"),
      "utf8",
    );

    assert.match(postinst, /case "\$\{1:-\}" in\s+configure\)/);
    assert.match(postinst, /if \[ -f "\$\{CONFIG\}" \]; then/);
    assert.match(postinst, /mihomo_sha256 =/);
    assert.match(postinst, /mv -f -- "\$\{config_temp\}" "\$\{CONFIG\}"/);
    assert.match(postinst, /trap 'exit 129' HUP/);
    assert.match(postinst, /systemctl restart iran-split-helper\.service/);
    assert.match(
      postinst,
      /systemctl is-active --quiet iran-split-helper\.service/,
    );
    assert.match(postinst, /getent passwd "\$\{SUDO_UID\}"/);
    assert.match(postinst, /install it from the app/);
    assert.match(prerm, /remove\|deconfigure\)/);
    assert.match(prerm, /upgrade\|failed-upgrade\|abort-upgrade/);
    assert.doesNotMatch(
      prerm,
      /upgrade\|failed-upgrade\|abort-upgrade\|\*\)[\s\S]*?systemctl (?:stop|disable)/,
    );
    assert.match(
      helperInstaller,
      /systemctl restart iran-split-helper\.service/,
    );
    assert.match(
      helperInstaller,
      /systemctl is-active --quiet iran-split-helper\.service/,
    );
    assert.match(
      helperInstaller,
      /mktemp \/etc\/iran-split\/helper\.toml\.XXXXXX/,
    );
    assert.match(helperInstaller, /trap 'exit 130' INT/);
    assert.match(
      helperInstaller,
      /mv -f -- "\$\{config_temp\}" "\$\{CONFIG_DEST\}"/,
    );
  });

  it("strips Windows carriage returns from the Rust toolchain read by build.sh", () => {
    const buildScript = readFileSync(join(root, "build.sh"), "utf8");
    assert.match(buildScript, /RUST_VERSION=.*\| tr -d '\\r'/);
  });

  it("gates Linux helper-install paths so Windows dead_code stays clean", () => {
    const source = readFileSync(
      join(root, "src-tauri/src/helper_install.rs"),
      "utf8",
    );
    assert.match(
      source,
      /#\[cfg\(target_os = "linux"\)\]\s*const LINUX_HELPER_ROOT/,
    );
    assert.match(
      source,
      /#\[cfg\(target_os = "linux"\)\]\s*fn helper_binary_candidates/,
    );
    assert.match(
      source,
      /#\[cfg\(target_os = "linux"\)\]\s*#\[must_use\]\s*pub\(crate\) fn parse_proc_status_ids/,
    );
    assert.match(
      source,
      /#\[cfg\(target_os = "linux"\)\]\s*use std::path::\{Path, PathBuf\};/,
    );
    assert.match(
      source,
      /#\[cfg\(target_os = "linux"\)\]\s*\{\s*let staging_dir[\s\S]*?let payload_dir = /,
    );
    assert.match(
      source,
      /#\[cfg\(target_os = "linux"\)\]\s*use std::os::unix::fs::PermissionsExt;/,
    );
  });

  it("stages the helper payload out of the AppImage FUSE mount for pkexec", () => {
    const source = readFileSync(
      join(root, "src-tauri/src/helper_install.rs"),
      "utf8",
    );
    // root cannot read /tmp/.mount_* without allow_other, so pkexec must be
    // handed copies rather than the paths inside the mount.
    assert.match(source, /\.arg\(&payload\.script\)/);
    assert.match(source, /\.arg\(&payload\.helper\)/);
    assert.match(source, /\.arg\(&payload\.mihomo\)/);
    assert.doesNotMatch(source, /\.arg\(&script\)/);
    assert.doesNotMatch(source, /\.arg\(&helper_src\)/);
    assert.doesNotMatch(source, /\.arg\(&mihomo_src\)/);
    // 126 is a dismissed polkit dialog; 127 is a real failure to execute.
    assert.match(source, /output\.status\.code\(\) == Some\(126\)/);
    assert.doesNotMatch(
      source,
      /Some\(126\)\s*\|\|\s*output\.status\.code\(\) == Some\(127\)/,
    );
    assert.match(source, /fs::Permissions::from_mode\(0o700\)/);
  });

  it("re-raises the elevated Windows exit code instead of PowerShell's", () => {
    const source = readFileSync(
      join(root, "src-tauri/src/helper_install.rs"),
      "utf8",
    );
    assert.match(source, /-Verb RunAs -Wait -PassThru/);
    assert.match(source, /exit \$process\.ExitCode/);
    assert.match(source, /\$ErrorActionPreference = 'Stop'/);
    assert.match(source, /const UAC_CANCELLED: i32 = 1223;/);
    // ADR 0030: the GUI is a windows-subsystem binary, so the elevation shell
    // must not flash a console over the UI.
    assert.match(source, /\.creation_flags\(CREATE_NO_WINDOW\)/);
    assert.match(source, /const CREATE_NO_WINDOW: u32 = 0x0800_0000;/);
    // Start-Process -Wait without -PassThru reports PowerShell's own status, so
    // a failed helper and a refused UAC prompt both looked like success.
    assert.doesNotMatch(source, /-Verb RunAs -Wait"/);
  });

  it("uses tail expressions in cfg blocks so host Clippy needless_return stays clean", () => {
    const cfgReturn = /#\[cfg\([^\]]+\)\]\s*\{\s*return /;
    const offenders = rustSources(root).filter((path) =>
      cfgReturn.test(readFileSync(path, "utf8")),
    );

    assert.deepEqual(
      offenders,
      [],
      "cfg blocks compiled by host Clippy must be tail expressions, not return statements",
    );
  });
});
