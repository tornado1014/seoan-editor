#!/usr/bin/env python3
"""고칠 문단 목록 → hlt-cli 편집 요청(--operations) JSON.

  python3 ops.py <input.HLT> <edits.json> > ops.json

edits.json = [{"paragraph_id": "paragraph:3", "replacement": "새 글"}, ...]
expected_revision(파일 SHA-256 앞 16자리의 10진수)과 expected_text_sha256(지금 문단 글 UTF-8 의 대문자 SHA-256)을 채운다.
"""

import hashlib
import json
import subprocess
import sys


def paragraphs(hlt):
    texts, cursor = {}, "0"
    while cursor is not None:
        out = subprocess.run(
            ["hlt-cli", "inspect", hlt, "--kind", "paragraphs", "--limit", "500", "--cursor", cursor],
            capture_output=True, text=True, check=False,
        )
        value = json.loads(out.stdout)
        if not value["ok"]:
            sys.exit(f"inspect 실패: {value['error']}")
        texts.update((item["id"], item["text"]) for item in value["result"]["items"])
        cursor = value["result"]["next_cursor"]
    return texts


def main():
    hlt, edits_path = sys.argv[1], sys.argv[2]
    with open(hlt, "rb") as f:
        revision = int(hashlib.sha256(f.read()).hexdigest()[:16], 16)
    with open(edits_path, encoding="utf-8") as f:
        edits = json.load(f)
    texts = paragraphs(hlt)
    operations = []
    for edit in edits:
        current = texts.get(edit["paragraph_id"])
        if current is None:
            sys.exit(f"없는 문단: {edit['paragraph_id']}")
        operations.append({
            "paragraph_id": edit["paragraph_id"],
            "expected_text_sha256": hashlib.sha256(current.encode("utf-8")).hexdigest().upper(),
            "replacement": edit["replacement"],
        })
    json.dump({"expected_revision": str(revision), "operations": operations}, sys.stdout, ensure_ascii=False)


if __name__ == "__main__":
    main()
