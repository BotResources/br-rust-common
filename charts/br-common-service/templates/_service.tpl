{{- /*
br-common-service.service — the ClusterIP Service, named after the service.

The port is named `http`: the BotResources GraphQL gateway builds each
subgraph URL from the Service's `http`-named port, so the name is part of the
contract. Discovery by the gateway is a label the service chart adds through
`commonLabels` (README.md).
*/ -}}
{{- define "br-common-service.service" -}}
apiVersion: v1
kind: Service
metadata:
  name: {{ include "br-common-service.name" . }}
  labels:
    {{- include "br-common-service.labels" . | nindent 4 }}
spec:
  type: ClusterIP
  selector:
    {{- include "br-common-service.selectorLabels" . | nindent 4 }}
  ports:
    - name: http
      port: {{ include "br-common-service.port" . }}
      targetPort: http
      protocol: TCP
{{- end -}}
