{{- /*
br-rust-common-chart.service — the ClusterIP Service, named after the service.

The port is named `http`: the BotResources GraphQL gateway builds each
subgraph URL from the Service's `http`-named port, so the name is part of the
contract. Discovery by the gateway is a label the service chart adds through
`commonLabels` (README.md).
*/ -}}
{{- define "br-rust-common-chart.service" -}}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "br-rust-common-chart.name" . }}
  labels:
    {{- include "br-rust-common-chart.labels" . | nindent 4 }}
spec:
  type: ClusterIP
  selector:
    {{- include "br-rust-common-chart.selectorLabels" . | nindent 4 }}
  ports:
    - name: http
      port: {{ include "br-rust-common-chart.port" . }}
      targetPort: http
      protocol: TCP
{{- end -}}
