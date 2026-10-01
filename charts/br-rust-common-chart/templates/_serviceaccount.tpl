{{- /*
br-rust-common-chart.serviceaccount — the service's own identity, named after the
service. Its token is not mounted (`automountServiceAccountToken: false` on
the pod) unless the service chart says the binary calls the Kubernetes API.
*/ -}}
{{- define "br-rust-common-chart.serviceaccount" -}}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "br-rust-common-chart.name" . }}
  labels:
    {{- include "br-rust-common-chart.labels" . | nindent 4 }}
{{- end -}}
