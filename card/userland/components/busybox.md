# busybox

Static build against the musl sysroot with `knc-cc`; `busybox.sh` is the
recipe (pinned 1.37.0, trust-on-first-use checksum, `defconfig` plus the
toggles listed in the script).

Applets removed from `defconfig`: `tc` (fights newer kernel UAPI
headers), `hwclock`/`rdate` (no RTC on the card), `iwconfig`, the DHCP
client and server (addresses are static, assigned by the host), console
font and framebuffer tools (no console hardware), NFS mount (no NFS in the
card kernel config for now), SELinux, and the SHA1/SHA256 hardware
acceleration paths (hand-written SHA-NI and SSE assembly with runtime
dispatch: 291 flagged instructions in the first build).

The only ABI-relevant code is `float` use in `awk`, `dc`, and `printf`,
all through the C ABI the compiler defines.

## Verification

The script audits the binary with `phi-isa-audit` (must be clean) and runs
it on the host: `busybox echo`, a shell arithmetic expression, `uname`,
and the applet count. A knc64-x87 binary runs unchanged on the host CPU,
so this is a real execution test of every code path those applets touch.
