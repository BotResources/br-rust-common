#!/usr/bin/env bash
# chart-release-lib.sh — the decisions behind a chart release, as functions.
#
# Sourced by check-chart.sh (the version gate of a pull request) and by
# .github/workflows/chart-release.yml (what a release run does), and proven
# case by case by test-chart-release.sh. No function here pushes, tags or
# writes outside a temporary directory.
#
# Convention: each function prints ONE line on stdout — its verdict, or the
# reason it refuses — and its exit status says which:
#   0  verdict (e.g. `present`, `push`, or the gate's OK message);
#   1  refused, or an error the caller must not guess past;
#   2  skipped (chart_version_gate only: no base to compare with).
#
# Needs bash 3.2+, git, yq (mikefarah v4); helm for the registry and package
# functions.

# The version shapes a release accepts: SemVer 2.0.0 without build metadata
# (an OCI tag cannot carry `+`).
CHART_SEMVER_RE='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$'

# semver_cmp A B — prints -1, 0 or 1: the SemVer 2.0.0 precedence of A
# against B. A pre-release sorts below its release (1.0.0-rc.1 < 1.0.0), and
# pre-release identifiers compare numerically when both are numeric, as ASCII
# otherwise, a numeric one below an alphanumeric one. `sort -V` gets the first
# rule wrong, hence this function.
semver_cmp() {
  local LC_ALL=C
  local a="${1%%+*}" b="${2%%+*}"
  local a_pre="" b_pre="" i x y
  case "$a" in *-*) a_pre="${a#*-}" ;; esac
  case "$b" in *-*) b_pre="${b#*-}" ;; esac
  local -a ac bc ap bp
  IFS=. read -ra ac <<<"${a%%-*}"
  IFS=. read -ra bc <<<"${b%%-*}"
  for i in 0 1 2; do
    x="${ac[$i]:-0}" y="${bc[$i]:-0}"
    if ((10#$x > 10#$y)); then echo 1; return 0; fi
    if ((10#$x < 10#$y)); then echo -1; return 0; fi
  done
  if [ -z "$a_pre" ] && [ -z "$b_pre" ]; then echo 0; return 0; fi
  if [ -z "$a_pre" ]; then echo 1; return 0; fi
  if [ -z "$b_pre" ]; then echo -1; return 0; fi
  IFS=. read -ra ap <<<"$a_pre"
  IFS=. read -ra bp <<<"$b_pre"
  i=0
  while :; do
    if [ "$i" -ge "${#ap[@]}" ] && [ "$i" -ge "${#bp[@]}" ]; then echo 0; return 0; fi
    if [ "$i" -ge "${#ap[@]}" ]; then echo -1; return 0; fi
    if [ "$i" -ge "${#bp[@]}" ]; then echo 1; return 0; fi
    x="${ap[$i]}" y="${bp[$i]}"
    if [[ "$x" =~ ^[0-9]+$ && "$y" =~ ^[0-9]+$ ]]; then
      if ((10#$x > 10#$y)); then echo 1; return 0; fi
      if ((10#$x < 10#$y)); then echo -1; return 0; fi
    elif [[ "$x" =~ ^[0-9]+$ ]]; then
      echo -1; return 0
    elif [[ "$y" =~ ^[0-9]+$ ]]; then
      echo 1; return 0
    elif [[ "$x" > "$y" ]]; then
      echo 1; return 0
    elif [[ "$x" < "$y" ]]; then
      echo -1; return 0
    fi
    i=$((i + 1))
  done
}

# chart_version_gate <chart dir> <base ref> <tag prefix>
#
# The pull-request rule: a change to a packaged file (anything under the chart
# directory but ci/) is a new chart release, so it needs a Chart.yaml version
#   - that is a release version (CHART_SEMVER_RE);
#   - that is greater than the version at the merge base (never the same,
#     never lower: a lower or reused number could name content the registry
#     already holds under it);
#   - whose tag <tag prefix><version> does not exist: a released version is
#     immutable, and the release run would refuse it after the merge.
# Tags are read from the local repository: fetch them first (CI checks out
# with fetch-depth 0, which fetches every tag).
chart_version_gate() {
  local chart_dir="$1" base="$2" tag_prefix="$3"
  local version previous merge_base changed tag
  version="$(yq '.version' "${chart_dir}/Chart.yaml")" || {
    echo "cannot read the version of ${chart_dir}/Chart.yaml"
    return 1
  }
  tag="${tag_prefix}${version}"
  if ! git rev-parse --verify --quiet "${base}^{commit}" >/dev/null; then
    echo "${base} not found; version gate skipped (not a pull-request checkout)"
    return 2
  fi
  merge_base="$(git merge-base HEAD "$base")" || {
    echo "HEAD and ${base} have no merge base"
    return 1
  }
  changed="$(git diff --name-only "$merge_base" -- "$chart_dir" | grep -v "^${chart_dir}/ci/" || true)"
  if [ -z "$changed" ]; then
    echo "no packaged file changed since ${base} (version ${version})"
    return 0
  fi
  if ! [[ "$version" =~ $CHART_SEMVER_RE ]]; then
    echo "${chart_dir}/Chart.yaml version '${version}' is not a release version (X.Y.Z or X.Y.Z-<pre-release>)"
    return 1
  fi
  if git cat-file -e "${merge_base}:${chart_dir}/Chart.yaml" 2>/dev/null; then
    previous="$(git show "${merge_base}:${chart_dir}/Chart.yaml" | yq '.version')"
    if [ "$previous" = "$version" ]; then
      echo "${chart_dir} changed but Chart.yaml version is still ${version}: a chart change is a chart release — bump the version and add a CHANGELOG.md entry"
      return 1
    fi
    if [ "$(semver_cmp "$version" "$previous")" != 1 ]; then
      echo "Chart.yaml version goes from ${previous} to ${version}: a new chart release needs a greater version"
      return 1
    fi
  fi
  if git rev-parse --quiet --verify "refs/tags/${tag}" >/dev/null; then
    echo "version ${version} is already released (tag ${tag}): a released version is immutable — bump the version"
    return 1
  fi
  if [ -z "${previous:-}" ]; then
    echo "${chart_dir} is new since ${base} (version ${version})"
  else
    echo "version ${previous} -> ${version}"
  fi
}

# chart_tag_state <remote> <tag> — `present` or `absent` on the remote.
# Any other outcome of the lookup (network, auth) is an error, never `absent`.
chart_tag_state() {
  local rc=0 err
  err="$(git ls-remote --exit-code --tags "$1" "refs/tags/$2" 2>&1 >/dev/null)" || rc=$?
  case "$rc" in
    0) echo present ;;
    2) echo absent ;;
    *)
      echo "cannot tell whether tag $2 exists on $1 (git ls-remote exit ${rc}): ${err//$'\n'/ }"
      return 1
      ;;
  esac
}

# chart_registry_state <oci repo> <chart name> <version> — `present` or
# `absent` on the registry. `absent` needs a POSITIVE not-found answer (the
# registry said the tag or repository does not exist); an authentication
# failure, a rate limit, a 5xx or a network error is an error, never
# `absent` — the caller would otherwise push over a published version, and an
# OCI tag on GHCR is mutable. Run it after `helm registry login`, so the answer
# is the registry's view for the publisher, not an anonymous reader's.
chart_registry_state() {
  local err
  if err="$(helm show chart "${1}/${2}" --version "$3" 2>&1 >/dev/null)"; then
    echo present
    return 0
  fi
  if grep -qiE ': not found[[:space:]]*$|manifest unknown|name unknown' <<<"$err" &&
    ! grep -qiE 'status code (401|403|429|5[0-9][0-9])|no such host|timeout|connection (refused|reset)|tls handshake|x509|denied|unauthorized' <<<"$err"; then
    echo absent
    return 0
  fi
  echo "cannot tell whether ${1}/${2}:${3} is published: ${err//$'\n'/ }"
  return 1
}

# chart_packages_match <package.tgz> <package.tgz> — `same` or `different`.
# Every file is compared byte for byte, except Chart.yaml, which `helm
# package` re-serialises (another Helm version may order or quote it
# differently): it is compared with sorted keys. The diff goes to stderr.
chart_packages_match() {
  local work d f rc=0
  work="$(mktemp -d)"
  mkdir -p "${work}/a" "${work}/b"
  if ! tar -xzf "$1" -C "${work}/a" || ! tar -xzf "$2" -C "${work}/b"; then
    rm -rf "$work"
    echo "cannot extract $1 or $2"
    return 1
  fi
  for d in a b; do
    for f in "${work}/${d}"/*/Chart.yaml; do
      if [ -f "$f" ]; then yq -i -P 'sort_keys(..)' "$f"; fi
    done
  done
  diff -r "${work}/a" "${work}/b" >&2 || rc=$?
  rm -rf "$work"
  case "$rc" in
    0) echo same ;;
    1) echo different ;;
    *)
      echo "cannot compare $1 with $2 (diff exit ${rc})"
      return 1
      ;;
  esac
}

# chart_publish_decision <tag: present|absent> <registry: present|absent>
#                        <content: same|different, or empty when absent>
#
# What a release run does with a version, given the git tag, the registry and,
# when the registry holds the version, whether the published package equals
# this commit's:
#
#   tag      registry  content    decision
#   present  present   same       noop  — released; nothing to do
#   present  present   different  REFUSED — released with other content
#   present  absent    -          REFUSED — a tag with no package behind it
#   absent   present   same       tag   — a previous run pushed, then failed
#   absent   present   different  REFUSED — published with other content
#   absent   absent    -          push  — then tag
#
# A published version is never pushed again, and a tag is never moved.
chart_publish_decision() {
  local tag="$1" registry="$2" content="${3:-}"
  case "${tag}/${registry}/${content}" in
    present/present/same) echo noop ;;
    absent/present/same) echo tag ;;
    absent/absent/) echo push ;;
    */present/different)
      echo "this version is already published with different content: a published version is immutable — bump the chart version"
      return 1
      ;;
    present/absent/)
      echo "the tag exists but the registry does not hold this version (a tag pushed by hand, or a deleted package): investigate before releasing — neither is ever repaired by moving the tag"
      return 1
      ;;
    *)
      echo "invalid inputs: tag='${tag}' registry='${registry}' content='${content}'"
      return 1
      ;;
  esac
}
