{{- /*
br-rust-common-chart.all — every resource of the topology, in one include:
ServiceAccount, Service, Deployment, and the PodDisruptionBudget and
NetworkPolicy when enabled. A service chart whose topology is exactly the
library's writes a single template:

  {{ include "br-rust-common-chart.all" . }}

and adds its own templates beside it for what only that service needs (a
bootstrap Job, a second Service).
*/ -}}
{{- define "br-rust-common-chart.all" -}}
{{ include "br-rust-common-chart.serviceaccount" . }}
---
{{ include "br-rust-common-chart.service" . }}
---
{{ include "br-rust-common-chart.deployment" . }}
{{- with include "br-rust-common-chart.pdb" . | trim }}
---
{{ . }}
{{- end }}
{{- with include "br-rust-common-chart.networkpolicy" . | trim }}
---
{{ . }}
{{- end }}
{{- end -}}
