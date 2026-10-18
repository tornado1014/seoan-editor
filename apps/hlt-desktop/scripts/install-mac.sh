#!/bin/bash
# 맥 앱으로 굽고 /Applications 에 설치한다(있으면 바꿔 끼운다): apps/hlt-desktop 에서 `scripts/install-mac.sh`
# - 서명은 ad-hoc(이 맥 전용 · 애플 공증 없음). 다른 맥에 줄 때는 .dmg 를 주되 처음 열 때 「우클릭 → 열기」가 필요하다.
# - 설치 뒤 LaunchServices 에 다시 알려 .HLT · .fin · .hlz 「다음으로 열기」에 뜨게 한다.
set -euo pipefail
cd "$(dirname "$0")/.."
npx tauri build --bundles app,dmg --config '{"bundle":{"createUpdaterArtifacts":false}}'  # 업데이트 파일 · 서명은 릴리스(Actions)만
NAME="서안 통특기.app"
SRC="../../target/release/bundle/macos/$NAME"
DST="/Applications/$NAME"
osascript -e 'quit app id "io.github.tornado1014.seoan"' 2>/dev/null || true  # 경로로 찾는 pkill 은 한글(NFD) 경로에서 못 찾는다
rm -rf "$DST"
ditto "$SRC" "$DST"
codesign --force --deep -s - "$DST"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$DST"
echo "설치함: $DST"
ls ../../target/release/bundle/dmg/*.dmg
