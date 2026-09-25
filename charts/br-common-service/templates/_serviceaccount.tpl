{{- /*
br-common-service.serviceaccount — the service's own identity, named after the
service. Its token is not mounted (`automountServiceAccountToken: false` on
the pod) unless the service chart says the binary calls the Kubernetes API.
*/ -}}
{{- define "br-common-service.serviceaccount" -}}
apiVersion: v1
kind: ServiceAccount
metadata:
  name: {{ include "br-common-service.name" . }}
  labels:
    {{- include "br-common-service.labels" . | nindent 4 }}
{{- end -}}
