{{- /*
br-common-service — the image reference, and the check that pairs the service
chart with the binary versions it can run. Reads the TOP-LEVEL `.Values` and
the `.Chart` of the SERVICE chart, like every helper (_helpers.tpl).
*/ -}}

{{- /*
THE SUPPORTED BINARY RANGE — the service versions the SERVICE chart can run.

Declared by the service chart as the Chart.yaml annotation
`botresources.ai/supported-app-versions`: a Masterminds semver constraint (the
syntax of Helm's `semverCompare` and of Kargo's `constraint:`). `.Chart` is
the service chart here, because the service chart includes these templates
with its own context. Same annotation as the runner charts (br-runner) and
br-svc-runners; the enforcement is this library's own
(br-common-service.imageTag): a digest is split off and the tag before it
checked, SemVer build metadata is refused, a version tag is always
range-checked, and `image.enforceSupportedVersions=false` lifts the check for
local-build tags only.
*/ -}}
{{- define "br-common-service.supportedRange" -}}
{{- $range := index (.Chart.Annotations | default dict) "botresources.ai/supported-app-versions" | default "" -}}
{{- required (printf "%s: the Chart.yaml annotation botresources.ai/supported-app-versions is required — it states which versions of the service binary this chart can run" .Chart.Name) $range -}}
{{- end -}}

{{- /*
The image tag the pod runs: `<tag>`, or `<tag>@sha256:<64 lowercase hex>` to
pin the digest as well. REQUIRED: the deploying repository pins it per
environment (Kargo writes it on promotion); the chart never picks a binary on
its own.
The checks, in order:

  1. A digest suffix `@sha256:<64 lowercase hex>` is split off and kept in the
     rendered reference. It is admitted whatever
     `image.enforceSupportedVersions` says (digest pinning is supply-chain
     hardening), and the tag before it is checked as if it were alone.
  2. The tag must be an OCI tag (`[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}`). SemVer
     build metadata (`+…`) is not one: it would render, then fail at the
     kubelet with InvalidImageName.
  3. A release version — SemVer 2.0 `MAJOR.MINOR.PATCH[-PRERELEASE]`, an
     optional leading `v`, no build metadata — must be inside the supported
     range, ALWAYS: a promotion's `helm template` step cannot pair the chart
     with a binary it cannot run, and no values file can lift that check.
     A pre-release is inside the range only if the range names a pre-release,
     e.g. `>=0.5.0-0 <0.6.0` (Masterminds/Kargo semantics).
  4. A tag that starts like a version (`v` then a digit, or a digit) but is
     not a full release version (`0.9`, `0.9.0_x`) fails, whatever the flag
     says.
  5. Any other tag (`latest` included) fails, unless
     `image.enforceSupportedVersions` is false AND the tag names a local
     build: it starts with `local-`, `local.`, `dev-` or `dev.` (e.g.
     `local-build`, `dev-<sha>`), and has no version to check.

The supported-range annotation is required whatever the tag, a local build
included: a chart that does not state what it can run is incomplete.

`else if` chains and a non-empty range on purpose: under `helm lint` a
missing tag or annotation reaches the checks as "", and `semverCompare` on ""
is a template panic.
*/ -}}
{{- define "br-common-service.imageTag" -}}
{{- $image := .Values.image | default dict -}}
{{- $tag := toString (required "image.tag is required: the service version to deploy, set per environment by the deploying repository (Kargo writes it on promotion)" $image.tag) -}}
{{- $range := include "br-common-service.supportedRange" . -}}
{{- $enforce := eq (include "br-common-service.flag" (dict "value" $image.enforceSupportedVersions "default" true "field" "image.enforceSupportedVersions")) "true" -}}
{{- $digest := regexFind "@sha256:[0-9a-f]{64}$" $tag -}}
{{- $version := trimSuffix $digest $tag -}}
{{- $core := "(0|[1-9][0-9]*)" -}}
{{- $preId := "(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)" -}}
{{- $release := printf "^v?%s\\.%s\\.%s(-%s(\\.%s)*)?$" $core $core $core $preId $preId -}}
{{- $supports := printf "The chart %s supports %s (Chart.yaml annotation botresources.ai/supported-app-versions)" .Chart.Name $range -}}
{{- if not $tag -}}
{{- /* Already reported by `required`. */ -}}
{{- else if not (regexMatch "^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$" $version) -}}
{{- fail (printf "image.tag %q is not an image tag: an OCI tag is 1-128 letters, digits, '_', '.' and '-', not starting with '.' or '-', optionally followed by @sha256:<64 lowercase hex>. SemVer build metadata ('+...') cannot appear in one" $tag) -}}
{{- else if regexMatch $release $version -}}
{{- if $range -}}
{{- if not (semverCompare $range $version) -}}
{{- fail (printf "image.tag %s is outside the range the chart %s %s supports: %s (Chart.yaml annotation botresources.ai/supported-app-versions). Deploy it with a chart version whose range covers %s" $tag .Chart.Name .Chart.Version $range $version) -}}
{{- end -}}
{{- end -}}
{{- else if regexMatch "^v?[0-9]" $version -}}
{{- fail (printf "image.tag %q is not a release version: a version tag is MAJOR.MINOR.PATCH[-PRERELEASE] (SemVer 2.0, optional leading v, no build metadata), and image.enforceSupportedVersions cannot lift that. %s" $tag $supports) -}}
{{- else if $enforce -}}
{{- fail (printf "image.tag %q is not a release version. %s; image.enforceSupportedVersions=false admits one other kind of tag only: a local build, whose tag starts with local-, local., dev- or dev." $tag $supports) -}}
{{- else if not (regexMatch "^(local|dev)[-.]" $version) -}}
{{- fail (printf "image.tag %q is neither a release version nor a local build tag: image.enforceSupportedVersions=false admits only a tag that starts with local-, local., dev- or dev. (e.g. local-build, dev-<sha>). %s" $tag $supports) -}}
{{- end -}}
{{- $tag -}}
{{- end -}}

{{- define "br-common-service.image" -}}
{{- $image := .Values.image | default dict -}}
{{- required "image.repository is required: the service image, without tag; set it in the service chart" $image.repository -}}:{{- include "br-common-service.imageTag" . -}}
{{- end -}}
