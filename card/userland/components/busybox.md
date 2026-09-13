# busybox

Static build against the musl sysroot with `knc-cc`. Default config minus
networking applets that need kernel features the card lacks (no wireless,
no ACPI tools), plus `udhcpc` off (addresses are static, assigned by the
host).

The only ABI-relevant code is `float` use in `awk`, `dc`, and `printf`,
all through the C ABI the compiler defines.
