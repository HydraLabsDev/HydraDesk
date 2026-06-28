<!--
Copyright (c) 2026 HydraCodeLabs
Owner: HydraCodeLabs
Project: HydraDesk
SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
Last updated: 2026-06-28T00:00:00Z
-->

# HydraDesk — Terms of Use

This is a plain-language summary of how you may use HydraDesk. It is written to be
easy to follow. **It is not the legal contract itself** — the binding terms are
the [PolyForm Noncommercial License 1.0.0](LICENSE). If anything here seems to
conflict with `LICENSE`, `LICENSE` wins.

In short: **HydraDesk is free for any noncommercial use. Commercial use needs a
separate license from HydraCodeLabs.**

---

## 1. What HydraDesk is

HydraDesk is a tool that configures a GNOME/Linux device so you can connect to its
real desktop from Windows over standard RDP (`mstsc`). You run it on hardware you
own or are authorized to administer.

## 2. What you may do (free, no payment, no account)

For **any noncommercial purpose**, you may:

- ✅ **Use** HydraDesk on as many of your own devices as you like.
- ✅ **Copy and share** it, as long as you include the `LICENSE` file and the
  copyright notice with every copy.
- ✅ **Modify** it and build your own versions for noncommercial use.
- ✅ Use it for **personal projects, home labs, study, research, and evaluation.**

"Noncommercial" means use that is **not primarily intended for or directed toward
commercial advantage or monetary compensation.** Personal use, hobby projects, and
use by charities and schools for their own noncommercial work all qualify.

## 3. What you may not do without a separate license

- ❌ **Sell** HydraDesk, or sell a product or service whose value depends on it.
- ❌ Use it **in or for a commercial business operation** (running it on company
  infrastructure to do company work counts as commercial use).
- ❌ Offer it as a **paid hosted/managed service.**
- ❌ Remove or hide the copyright notice or the `Required Notice` line.
- ❌ Use the **HydraDesk or HydraCodeLabs names, logos, or branding** to imply
  endorsement, or as your own product's branding. The license covers the code,
  **not** the trademarks.

Want to do any of the above? See [Commercial licensing](#7-commercial-licensing).

## 4. No warranty — use at your own risk

HydraDesk is provided **"as is", with no warranty of any kind.** HydraCodeLabs is
not liable for any damage, data loss, downtime, or security incident arising from
its use, to the maximum extent the law allows. You are responsible for testing it
on your own systems before relying on it.

## 5. Your security responsibilities

HydraDesk enables remote desktop access and, on auto-login devices, stores an RDP
credential locally. By using it you agree that **you are responsible for operating
it safely**, including:

- **Keep RDP on your LAN.** Do **not** port-forward port 3389 to the internet.
  Reach the device over a VPN / Tailscale / WireGuard / SSH tunnel instead.
- **Use full-disk encryption** on auto-login devices, so the stored RDP credential
  is protected if the device is lost or stolen.
- **Keep the system patched**, use strong passwords, and physically secure the
  device.

HydraDesk applies sensible defaults (LAN-scoped, rate-limited firewall rule; weak
passwords refused; a public-IP warning), but the final security of your deployment
is your responsibility.

## 6. Third-party components

HydraDesk builds on open-source software (GNOME Remote Desktop and the Rust crates
listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)). Those components keep
their own licenses and copyright holders; nothing here changes them.

## 7. Commercial licensing

If you want to use HydraDesk commercially — bundle it, resell it, run it as part of
a business operation, or offer it as a paid service — contact **HydraCodeLabs** to
arrange a commercial license. The copyright holder retains all rights not granted
by the PolyForm Noncommercial License.

> Project home: <https://github.com/HydraLabsDev/HydraDesk>

---

*This summary is provided for convenience only and does not modify the
[PolyForm Noncommercial License 1.0.0](LICENSE), which is the governing agreement.*
