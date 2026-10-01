{{- /*
br-rust-common-chart.networkpolicy — an additive NetworkPolicy on the service's
pods, rendered only when `networkPolicy.enabled` is true (default false).

The rules are the deploying environment's topology (where the object store,
the database or the gateway run), so the library writes none: it takes
`networkPolicy.ingress` and/or `networkPolicy.egress` verbatim, and each key
that is present adds its direction to `policyTypes`. A present but empty list
denies that direction.
*/ -}}
{{- define "br-rust-common-chart.networkpolicy" -}}
{{- $np := .Values.networkPolicy | default dict -}}
{{- if eq (include "br-rust-common-chart.flag" (dict "value" $np.enabled "default" false "field" "networkPolicy.enabled")) "true" -}}
{{- $types := list -}}
{{- if hasKey $np "ingress" }}{{ $types = append $types "Ingress" }}{{ end -}}
{{- if hasKey $np "egress" }}{{ $types = append $types "Egress" }}{{ end -}}
{{- if not $types -}}
{{- fail "networkPolicy.enabled is true but neither networkPolicy.ingress nor networkPolicy.egress is set" -}}
{{- end -}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ include "br-rust-common-chart.name" . }}
  labels:
    {{- include "br-rust-common-chart.labels" . | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      {{- include "br-rust-common-chart.selectorLabels" . | nindent 6 }}
  policyTypes:
    {{- toYaml $types | nindent 4 }}
  {{- if hasKey $np "ingress" }}
  ingress:
    {{- $np.ingress | default list | toYaml | nindent 4 }}
  {{- end }}
  {{- if hasKey $np "egress" }}
  egress:
    {{- $np.egress | default list | toYaml | nindent 4 }}
  {{- end }}
{{- end -}}
{{- end -}}
