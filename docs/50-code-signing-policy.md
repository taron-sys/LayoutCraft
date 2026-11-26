# 50. Code signing policy (draft)

> **Draft, not in effect. LayoutCraft releases are not code-signed today.** This text is prepared for an application to
> SignPath Foundation. Do not publish it as the project's policy, or repeat the sentence under "Signing service" as a fact,
> before the application is accepted. The items marked **TO CONFIRM** are the owner's to check.

## Signing service

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org)

The publisher Windows shows for a signed file is SignPath Foundation.

## What is signed

| File | Product name | Version |
| --- | --- | --- |
| `layoutcraft-app.exe` (x64 and ARM64) | LayoutCraft | the release version |
| `layoutcraft-mcp.exe` (x64 and ARM64) | LayoutCraft | the release version |
| `LayoutCraft-<version>-windows-<architecture>.msi` | LayoutCraft | the release version (numbers only in the MSI's own version field) |

- Only files built from this repository's source by its release workflow (`.github/workflows/release.yml`), from a version
  tag, are signed. Nothing built on a personal computer is signed.
- Third-party libraries are compiled into the programs from their published sources; no third-party program is signed on its
  own. They are listed with their licences in [THIRD-PARTY-NOTICES.txt](../THIRD-PARTY-NOTICES.txt).
- The build is the public one (`LAYOUTCRAFT_PUBLIC_BUILD=1`): it contains no private module.

## Team roles

| Role | What it may do | Members |
| --- | --- | --- |
| Authors (committers) | change the source without a further review | [@taron-sys](https://github.com/taron-sys) **TO CONFIRM** (anyone else with write access) |
| Reviewers | review every change proposed by someone who is not an author | [@taron-sys](https://github.com/taron-sys) **TO CONFIRM** |
| Approvers | approve each signing request | [@taron-sys](https://github.com/taron-sys) **TO CONFIRM** |

- Every member uses multi-factor authentication for GitHub and for SignPath. **TO CONFIRM**
- Each release is approved for signing by hand.

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user or the
person installing or operating it.

- The desktop app opens and saves files on the computer it runs on. It makes no network connection of its own: no telemetry,
  no update check, no upload.
- `layoutcraft-mcp` talks to the program that starts it, over standard input and output, and opens no network port. It reads
  and writes files only below the folder it is given. If you connect it to an AI assistant, what the assistant reads from
  your drawings is handled by that assistant, under its provider's privacy policy.

## Installing, changes to your system, removing

- The MSI installs for the current user, without administrator rights, into `%LOCALAPPDATA%\Programs\LayoutCraft`, and adds a
  Start menu shortcut. Offering LayoutCraft under "Open with" for `.vsdx` files is an option that is off unless you choose it.
- To remove it, use Windows Settings > Apps > Installed apps > LayoutCraft > Uninstall.
- The portable zip installs nothing; delete its folder to remove it.
- The app remembers the opened stencils and the chosen palette in `%APPDATA%\LayoutCraft`. Delete that folder to remove them.

## Reporting a problem

If you think a signed file is not what this policy describes, report it privately as set out in [SECURITY.md](../SECURITY.md).

## Before applying (checklist, remove when this policy takes effect)

- [ ] Read the current [SignPath Foundation conditions](https://signpath.org/terms.html) (the page said it was a draft when
      read on 2026-11-26). Among them: an OSI-approved licence for every component, no proprietary component, an actively
      maintained project that is already released, and a reputation that can be checked.
- [ ] **Possible obstacle:** the desktop app bundles the default fonts of its UI toolkit (`epaint_default_fonts`). One of them
      is under the Ubuntu Font Licence 1.0, which as far as we know is not on the OSI list. Ask SignPath whether a font
      licence like this is accepted; if not, replace the bundled fonts.
- [ ] Confirm the team roles and multi-factor authentication above.
- [ ] Protect the release tags (`v*`) so that only approvers can create them.
- [ ] Check the uninstall steps on a Windows machine.
- [ ] After acceptance: replace the `signtool` steps of the release workflow (`tools/packaging/windows/sign.ps1`) with
      SignPath's, and restrict product name and version there.
- [ ] After acceptance: remove the draft notice and this checklist, add a "Code signing policy" section to the front page that
      links here, and add the same link to the release notes. The front page text:

```markdown
## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).
[Roles, privacy and what is signed](docs/50-code-signing-policy.md).
```
