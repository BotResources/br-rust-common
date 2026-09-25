{{- /*
br-common-service.pdb — the PodDisruptionBudget, rendered only when
`podDisruptionBudget.enabled` is true (default false). Exactly one of
`minAvailable` / `maxUnavailable` is required, and a budget that no eviction
can ever satisfy fails the render: `minAvailable` (as a number) at or above
`replicaCount`, or `maxUnavailable: 0`, would hang every node drain forever.
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
{{- if and $hasMin (not (kindIs "string" $pdb.minAvailable)) (ge (int $pdb.minAvailable) $replicas) -}}
{{- fail (printf "podDisruptionBudget.minAvailable %d with replicaCount %d leaves no pod evictable: every node drain would hang. Lower it, or disable the budget" (int $pdb.minAvailable) $replicas) -}}
{{- end -}}
{{- if and $hasMax (not (kindIs "string" $pdb.maxUnavailable)) (eq (int $pdb.maxUnavailable) 0) -}}
{{- fail "podDisruptionBudget.maxUnavailable 0 leaves no pod evictable: every node drain would hang" -}}
{{- end -}}
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: {{ include "br-common-service.name" . }}
  labels:
    {{- include "br-common-service.labels" . | nindent 4 }}
spec:
  {{- if $hasMin }}
  minAvailable: {{ $pdb.minAvailable }}
  {{- else }}
  maxUnavailable: {{ $pdb.maxUnavailable }}
  {{- end }}
  selector:
    matchLabels:
      {{- include "br-common-service.selectorLabels" . | nindent 6 }}
{{- end -}}
{{- end -}}
