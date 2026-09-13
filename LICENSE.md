# License

## Project code (default)

MIT License

Copyright (c) 2026 Lasimeri

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Exceptions

- `card/kernel/` (patches, config fragments, platform code) is derived from and
  intended to be applied to the Linux kernel. It is licensed GPL-2.0-only, the
  same as Linux. See the SPDX header in each file.
- `card/drivers/` (out-of-tree kernel modules for the card) is GPL-2.0-only.
- Third-party projects that this repository patches (musl, tcc, QuickJS,
  CPython, LLVM) keep their own licenses. Only the patches are in this
  repository, and each patch carries the license of the project it modifies.
- Intel documents, MPSS archives, firmware, and Intel's kernel tree are never
  redistributed by this repository. `scripts/fetch-vendor.sh` downloads them
  from their public locations for local reference only.
