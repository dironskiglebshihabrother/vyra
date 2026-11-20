# Plugin exception (draft)

VYRA is licensed under the GNU General Public License, version 3. This file adds one permission on top of it, as section 7 of the license allows.

## Additional permission

A plugin is a separate program that talks to VYRA only through the published plugin API: the WIT interfaces in `sdk/wit` and the UI plugin SDK in `packages/plugin-sdk`.

If a plugin uses nothing from VYRA except that API, the copyright holders of VYRA do not treat it as a derivative work of VYRA. You may distribute such a plugin under any license, including a proprietary one.

This permission does not cover code copied from VYRA into a plugin, and it does not cover changes to VYRA itself.

## Status

This wording is a draft. The plugin API does not exist yet, see [docs/architecture/plugins.md](docs/architecture/plugins.md). A lawyer should check the text before the first plugin SDK is released.
