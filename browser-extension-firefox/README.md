# CHRONOS Browser Extension — Firefox / Zen variant

Firefox-compatible Manifest V3 variant (event-page background instead of a service
worker). The background/options/pairing logic is shared verbatim with the Chromium
build.

## Temporary installation (development verification)

1. Firefox or Zen: open `about:debugging#/runtime/this-firefox`
2. **Load Temporary Add-on…** → select this folder's `manifest.json`
3. Approve the prompt. Right after installation, the extension opens the local
   CHRONOS pairing page (`http://127.0.0.1:53711/pair`); its content script stores
   the pairing token automatically — no manual token copying. The same page can
   always be reopened by clicking the CHRONOS toolbar icon.

Temporary add-ons are removed when the browser closes — this is a development
method only, not an end-user solution.

## Permanent installation — what works locally, and the external blocker

The complete packaging work can be done locally:

```powershell
npm install -g web-ext
web-ext build          # produces chronos_website_attribution-1.0.0.zip in ./web-ext-artifacts/
web-ext lint           # validates the package against Mozilla requirements
```

**Blocker:** permanently installable Firefox add-ons must be **signed by Mozilla**
(`web-ext sign`), which requires free AMO developer credentials (an addons.mozilla.org
account and an API key). Those credentials do not exist on this machine and cannot be
invented. Until signing happens:

- `web-ext build` produces the exact package that would be submitted,
- `web-ext lint` passes is the pre-submission check to run,
- Unsigned `.xpi` files install permanently only in Firefox ESR/Developer Edition
  builds with `xpinstall.signatures.required = false`, which is not a normal
  end-user configuration.

CHRONOS ships this folder ready-to-sign; the signing step is the only remaining
external dependency and must be reported as such.

## Zen Browser

Zen is Firefox-based: load this variant the same way (temporary add-on for
development; signed build for permanent use).
