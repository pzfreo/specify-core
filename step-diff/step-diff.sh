#!/usr/bin/env bash
# The STEP diff action's work (action.yml): install specify-core, compare each changed STEP
# file with the base branch's, publish the pictures and write the comment.
set -euo pipefail

if [ -z "${PR:-}" ]; then
  echo "::error::The STEP diff action runs on pull_request events."
  exit 1
fi
if [ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]; then
  echo "::error::The STEP diff action runs on ubuntu-latest (Linux x86_64)."
  exit 1
fi

# The release: the one asked for, the action's own tag, or the latest.
if [ -z "$VERSION" ]; then
  case "$ACTION_REF" in
    v[0-9]*) VERSION=$ACTION_REF ;;
    *) VERSION=$(gh release list -R pzfreo/specify-core-rust -L 1 --json tagName --jq '.[0].tagName') ;;
  esac
fi
name="specify-core-$VERSION-x86_64-unknown-linux-gnu"
gh release download "$VERSION" -R pzfreo/specify-core-rust -p "$name.tar.gz" -p "$name.tar.gz.sha256" -D "$RUNNER_TEMP"
(cd "$RUNNER_TEMP" && sha256sum -c "$name.tar.gz.sha256" && tar -xzf "$name.tar.gz")
PATH="$RUNNER_TEMP/$name:$PATH"

# The STEP files changed, and those whose answers changed.
outs="$RUNNER_TEMP/step-diff"
mkdir -p "$outs"
{
  git diff --name-only --diff-filter=AM "$BASE" "$HEAD" -- '*.step' '*.stp' '*.STEP' '*.STP'
  git diff --name-only "$BASE" "$HEAD" -- '*.answers.json' | while read -r a; do
    for f in "${a%.answers.json}".{step,stp,STEP,STP}; do [ -f "$f" ] && echo "$f"; done
  done
} | sort -u > "$RUNNER_TEMP/changed"
if [ ! -s "$RUNNER_TEMP/changed" ]; then
  echo "No STEP file changed."
  exit 0
fi

while read -r step; do
  stem="${step%.*}"
  out="$outs/$(echo "$stem" | tr '/' '_')"
  mkdir -p "$out"
  echo "$step" > "$out/step"
  if ! git cat-file -e "$BASE:$step" 2>/dev/null; then
    touch "$out/added"
    continue
  fi
  git show "$BASE:$step" > "$out/old.step"
  # The answers as they were on the base branch are the ones to carry.
  answers=()
  if git cat-file -e "$BASE:$stem.answers.json" 2>/dev/null; then
    git show "$BASE:$stem.answers.json" > "$out/answers.old.json"
    answers=(--answers "$out/answers.old.json")
  fi
  specify-core diff "$out/old.step" "$step" "${answers[@]}" --title "$step" -o "$out"
done < "$RUNNER_TEMP/changed"

# The pictures, on the step-diff-images branch, for the comment to show.
dir="pr-$PR/${HEAD:0:7}"
git worktree add -q --detach "$RUNNER_TEMP/images"
(
  cd "$RUNNER_TEMP/images"
  if git ls-remote --exit-code origin step-diff-images > /dev/null; then
    git fetch -q origin step-diff-images && git checkout -q -B step-diff-images FETCH_HEAD
  else
    git checkout -q --orphan step-diff-images && git rm -rq --cached . && git clean -fdq
  fi
  mkdir -p "$dir"
  for out in "$outs"/*/; do
    [ -f "$out/model.png" ] && cp "$out/model.png" "$dir/$(basename "$out").png"
  done
  git add "$dir" 2> /dev/null || true
  if git -c user.name=github-actions -c user.email=github-actions@users.noreply.github.com \
    commit -qm "Pictures for #$PR at ${HEAD:0:7}"; then
    git push -q origin step-diff-images
  fi
)

# One comment, found by its marker and rewritten on each push.
marker='<!-- step-diff -->'
body="$RUNNER_TEMP/comment.md"
{
  echo "$marker"
  for out in "$outs"/*/; do
    step=$(cat "$out/step")
    if [ -f "$out/added" ]; then
      printf '### %s: Specify\n\nA new part: nothing to compare it with.\n\n' "$step"
      continue
    fi
    image="https://github.com/$REPO/blob/step-diff-images/$dir/$(basename "$out").png?raw=true"
    sed "s#](model.png)#]($image)#" "$out/report.md"
    if [ -s "$out/answers.json" ] && [ "$(cat "$out/answers.json")" != "{}" ]; then
      printf '\n<details><summary>The answers carried to the new revision (<code>%s.answers.json</code>)</summary>\n\n```json\n' "${step%.*}"
      cat "$out/answers.json"
      printf '\n```\n\n</details>\n'
    fi
    echo
  done
  echo "<sub>Compared by [specify-core](https://github.com/pzfreo/specify-core-rust) $VERSION at ${HEAD:0:7}.</sub>"
} > "$body"
id=$(gh api "repos/$REPO/issues/$PR/comments" --paginate \
  --jq ".[] | select(.body | startswith(\"$marker\")) | .id" | head -1)
if [ -n "$id" ]; then
  gh api -X PATCH "repos/$REPO/issues/comments/$id" -F body=@"$body" > /dev/null
else
  gh api -X POST "repos/$REPO/issues/$PR/comments" -F body=@"$body" > /dev/null
fi
