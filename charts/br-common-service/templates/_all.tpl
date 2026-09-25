{{- /*
br-common-service.all — every resource of the topology, in one include:
ServiceAccount, Service, Deployment, and the PodDisruptionBudget and
NetworkPolicy when enabled. A service chart whose topology is exactly the
library's writes a single template:

  {{ include "br-common-service.all" . }}

and adds its own templates beside it for what only that service needs (a
bootstrap Job, a second Service).
*/ -}}
{{- define "br-common-service.all" -}}
{{ include "br-common-service.serviceaccount" . }}
---
{{ include "br-common-service.service" . }}
---
{{ include "br-common-service.deployment" . }}
{{- with include "br-common-service.pdb" . | trim }}
---
{{ . }}
{{- end }}
{{- with include "br-common-service.networkpolicy" . | trim }}
---
{{ . }}
{{- end }}
{{- end -}}
