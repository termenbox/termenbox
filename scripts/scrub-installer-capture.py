#!/usr/bin/env python3
"""Makes a raw installer capture safe to commit (course step 3.1; called by capture-installer-output.sh).

    scrub-installer-capture.py RAW TIMING_IN TYPESCRIPT TIMING_OUT

RAW is `script --log-out`, TIMING_IN its `--log-timing` ("delay bytes" lines). It
  1. drops the `Script started` and `Script done` lines, which are not in the timing,
  2. replaces the claim secret by a fixed example, replaces every public IPv4 address in the pairing link's bundle by one of the documentation range 198.51.100.0/24
     (same length, so the bundle's length prefixes stay valid) and draws the QR again for the new link,
  3. rewrites the timing to the new byte counts,
  4. fails if the link still holds an address outside 192.0.2.0/24, 198.51.100.0/24 or 203.0.113.0/24, or if a dotted quad
     is left in the text.
The signature inside the bundle no longer matches after step 2: the link is an example and is shown as one. Needs the
`qrcode` package (pip install qrcode); set CAPTURE_PYTHON to a python that has it.
"""
import base64
import re
import sys

import qrcode

DOC = [re.compile(r"^192\.0\.2\.\d+$"), re.compile(r"^198\.51\.100\.\d+$"), re.compile(r"^203\.0\.113\.\d+$")]
raw_path, timing_in, ts_out, timing_out = sys.argv[1:5]
raw = open(raw_path, "rb").read()
timing = [(float(a), int(b)) for a, b in (l.split() for l in open(timing_in).read().splitlines() if l.strip())]

# 1. the header line and the footer line are not part of the timed output
raw = raw[raw.index(b"\n") + 1 :]
raw = raw[: raw.rindex(b"\nScript done")]
if sum(n for _, n in timing) != len(raw):
    sys.exit(f"timing covers {sum(n for _, n in timing)} bytes but the output has {len(raw)}")

# 2. the pairing block: QR rows, then "Scan this" and the link line
m = re.search(rb"(https://[^\s/]+/#/pair/)([A-Za-z0-9_-]+)(\?claim=[A-Za-z0-9_-]+)?", raw)
if not m:
    sys.exit("no pairing link in the capture")
payload = m.group(2).decode()
bundle = base64.urlsafe_b64decode(payload + "=" * (-len(payload) % 4))
counter = [0]

def standin(match):
    """198.51.100.<n> padded with zeros to the length of the address it replaces (184.174.37.227 -> 198.51.100.100)."""
    ip = match.group(0)
    if any(r.match(ip.decode()) for r in DOC):
        return ip
    counter[0] += 1
    base, host = b"198.51.100.", str(counter[0]).encode()
    pad = len(ip) - len(base) - len(host)
    if pad < 0:
        sys.exit(f"cannot stand in for {ip.decode()} with the same length; extend standin()")
    return base + host + b"0" * pad

new_bundle = re.sub(rb"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b", standin, bundle)
for ip in re.findall(rb"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}", new_bundle):
    if not any(r.match(ip.decode()) for r in DOC):
        sys.exit(f"an address is left in the bundle: {ip.decode()}")
new_payload = base64.urlsafe_b64encode(new_bundle).decode().rstrip("=")
# The claim secret of the throwaway server is replaced too: a fixed example of the same length (43 characters).
claim = b"?claim=" + b"EXAMPLE".ljust(len(m.group(3)) - len(b"?claim="), b"0") if m.group(3) else b""
link = m.group(1) + new_payload.encode() + claim

qr = qrcode.QRCode(error_correction=qrcode.constants.ERROR_CORRECT_L, border=4)
qr.add_data(link.decode())
qr.make(fit=True)
grid = qr.get_matrix()  # True = dark module, with the quiet zone
if len(grid) % 2:
    grid.append([True] * len(grid[0]))  # the missing half row is blank (dark), as the server draws it
rows = []
for y in range(0, len(grid), 2):
    row = ""
    for top, bottom in zip(grid[y], grid[y + 1]):
        # light modules are drawn as full blocks (dark terminal), dark ones as spaces: the server's Dense1x2, light on dark
        row += {(True, True): " ", (True, False): "▄", (False, True): "▀", (False, False): "█"}[(top, bottom)]
    rows.append(row)
block = "\r\n".join(rows).encode() + b"\r\n"

qr_start = raw.index("█".encode())
qr_end = raw.index(b"Scan this")
old_link_start, old_link_end = m.start(), m.end()
out = raw[:qr_start] + block + raw[qr_end:old_link_start] + link + raw[old_link_end:]

# 3. the timing: map each chunk boundary of the old stream to the new one
edits = [(qr_start, qr_end, len(block)), (old_link_start, old_link_end, len(link))]
def remap(off):
    shift = 0
    for start, end, new_len in edits:
        if off >= end:
            shift += new_len - (end - start)
        elif off > start:
            return start + shift + round((off - start) * new_len / (end - start))
    return off + shift
bounds, acc = [], 0
for delay, n in timing:
    bounds.append((delay, remap(acc), remap(acc + n)))
    acc += n
lines = [f"{d:.6f} {b - a}" for d, a, b in bounds if b > a]
open(timing_out, "w").write("\n".join(lines) + "\n")
open(ts_out, "wb").write(out)

# 4. nothing real is left in the text
text = out.decode("utf-8", "replace")
for ip in re.findall(r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}", text):
    sys.exit(f"a dotted quad is left in the typescript: {ip}")
width = max(len(r) for r in rows)
print(f"scrubbed: {len(rows)} QR rows, {width} columns; {counter[0]} public address(es) replaced")
