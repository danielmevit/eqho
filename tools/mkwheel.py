"""Build a self-contained pywhispercpp wheel by adding the 5 plain DLLs at the
wheel ROOT (they install next to _pywhispercpp.pyd in site-packages, and Windows
searches a loaded module's own dir for its dependencies — the proven-working
co-location mechanism). RECORD is updated so pip tracks/uninstalls them cleanly.
Usage: python3 mkwheel.py <in_wheel> <dll_dir> <out_wheel>"""
import sys, os, zipfile, hashlib, base64

wheel_in, dll_dir, wheel_out = sys.argv[1], sys.argv[2], sys.argv[3]
DLLS = ["whisper.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll", "ggml-vulkan.dll"]

def rec_hash(data):
    return "sha256=" + base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()

with zipfile.ZipFile(wheel_in) as z:
    names = z.namelist()
    record_name = next(n for n in names if n.endswith(".dist-info/RECORD"))
    record = z.read(record_name).decode()
    payload = {n: z.read(n) for n in names if n != record_name}

dll_data, new_lines = {}, []
for d in DLLS:
    with open(os.path.join(dll_dir, d), "rb") as f:
        data = f.read()
    dll_data[d] = data
    new_lines.append(f"{d},{rec_hash(data)},{len(data)}")

out = []
for ln in record.strip().splitlines():
    if ln.startswith(record_name + ","):
        out.extend(new_lines)
    out.append(ln)
new_record = "\n".join(out) + "\n"

os.makedirs(os.path.dirname(wheel_out), exist_ok=True)
with zipfile.ZipFile(wheel_out, "w", zipfile.ZIP_DEFLATED) as z:
    for n, data in payload.items():
        z.writestr(n, data)
    for d in DLLS:
        z.writestr(d, dll_data[d])
    z.writestr(record_name, new_record)

print("WHEEL_WRITTEN", os.path.getsize(wheel_out) // 1024, "KB")
with zipfile.ZipFile(wheel_out) as z:
    print("DLLS_IN_WHEEL", sum(1 for n in z.namelist() if n in DLLS))
