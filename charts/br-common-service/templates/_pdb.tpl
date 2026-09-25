{{- /*
br-common-service.pdb — the PodDisruptionBudget, rendered only when
`podDisruptionBudget.enabled` is true (default false). Exactly one of
`minAvailable` / `maxUnavailable` is required, each an integer or a
percentage, and a budget that no eviction can ever satisfy fails the render —
it would hang every node drain forever:
  - `minAvailable` at or above `replicaCount`, a percentage counted as
    Kubernetes counts it (rounded up: 100%, or 60% of 1 pod, keeps every pod).
    Checked only when `replicaCount` is above 0: with no pod, there is nothing
    to evict, so an environment scaled to 0 may keep its budget;
  - `maxUnavailable` 0 or 0%, at any `replicaCount`: it never allows an
    eviction once a pod runs.
A quoted integer ("1") is read as the integer, so the guard cannot be passed
by quoting.
*/ -}}
{{- define "br-common-service.pdb" -}}
{{- $pdb := .Values.podDisruptionBudget | default dict -}}
{{- if eq (include "br-common-service.flag" (dict "value" $pdb.enabled "default" false "field" "podDisruptionBudget.enabled")) "true" -}}
{{- $hasMin := not (kindIs "invalid" $pdb.minAvailable) -}}
{{- $hasMax := not (kindIs "invalid" $pdb.maxUnavailable) -}}
{{- if eq $hasMin $hasMax -}}
{{- fail "podDisruptionBudget: set exactly one of minAvailable and maxUnavailable" -}}
{{- end -}}
{{- $replicas := int (include "br-common-service.replicas" .) -}}
{{- $field := ternary "minAvailable" "maxUnavailable" $hasMin -}}
{{- $bound := include "br-common-service.pdbBound" (dict "value" (ternary $pdb.minAvailable $pdb.maxUnavailable $hasMin) "field" (printf "podDisruptionBudget.%s" $field)) -}}
{{- $percent := hasSuffix "%" $bound -}}
{{- $n := int (trimSuffix "%" $bound) -}}
{{- /* The pods the budget counts, rounded up for a percentage as Kubernetes does. */ -}}
{{- $pods := ternary (div (add (mul $replicas $n) 99) 100) $n $percent -}}
{{- if and $hasMin (gt $replicas 0) (ge (int $pods) $replicas) -}}
{{- fail (printf "podDisruptionBudget.minAvailable %s with replicaCount %d keeps %d pod(s) available and leaves no pod evictable: every node drain would hang. Lower it, or disable the budget" $bound $replicas (int $pods)) -}}
{{- end -}}
{{- if and $hasMax (eq $n 0) -}}
{{- fail (printf "podDisruptionBudget.maxUnavailable %s leaves no pod evictable, at any replicaCount: once a pod runs, every node drain would hang. Raise it, or disable the budget" $bound) -}}
{{- end -}}
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: {{ include "br-common-service.name" . }}
  labels:
    {{- include "br-common-service.labels" . | nindent 4 }}
spec:
  {{ $field }}: {{ ternary ($bound | quote) $n $percent }}
  selector:
    matchLabels:
      {{- include "br-common-service.selectorLabels" . | nindent 6 }}
{{- end -}}
{{- end -}}

{{- /*
A PodDisruptionBudget bound, normalised to "<n>" or "<p>%": an integer (a
YAML number or a quoted "2") or a percentage from 0% to 100% (the range
Kubernetes accepts). Anything else fails the render, a leading zero included:
sprig's `int` would read "010" as octal. Call with
(dict "value" <v> "field" "<values path>").
*/ -}}
{{- define "br-common-service.pdbBound" -}}
{{- $value := toString .value -}}
{{- if regexMatch "^(0|[1-9][0-9]*)$" $value -}}
{{- int $value -}}
{{- else if regexMatch "^(0|[1-9][0-9]*)%$" $value -}}
{{- $p := int (trimSuffix "%" $value) -}}
{{- if gt $p 100 -}}
{{- fail (printf "%s %s is above 100%%" .field $value) -}}
{{- end -}}
{{- $p -}}%
{{- else -}}
{{- fail (printf "%s must be an integer or a percentage (e.g. 1 or \"50%%\"), got %q" .field $value) -}}
{{- end -}}
{{- end -}}
