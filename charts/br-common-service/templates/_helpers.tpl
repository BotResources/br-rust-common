{{- /*
br-common-service — helpers.

Every named template reads the TOP-LEVEL `.Values` of the context it is given:
the service chart includes them with its own root context (`.`), so its
values.yaml and the deploying repository's per-environment values are what
these templates see. A library chart's own values.yaml would land under
`.Values.br-common-service` of the service chart and never reach them, so
every default of this library is coded HERE, in the templates, and documented
in README.md.

Two kinds of values, never mixed (README.md, "The ops contract"):
  - what the service binary defines — its name, image repository, port, probe
    paths, database name, the environment variables it reads — is set by the
    service chart, once, for every environment;
  - what differs per environment — `env`, `image.tag`, `replicaCount`,
    `resources`, `postgres.host`, `postgres.port`, `postgres.trustedNetwork`
    (and `postgres.sslMode` when it is false), the DSN Secret names,
    `nats.url` — has NO default here: a missing value fails the render with
    a message that names it. (Optional per-environment
    settings — scheduling, a PodDisruptionBudget — are simply absent unless
    set.)

Under `helm lint`, `required` and `fail` report instead of stopping the
render, so no helper may panic on an empty value it has just reported (a
panic would bury the clean message).
*/ -}}

{{- /* ── Identity ─────────────────────────────────────────────────────────── */ -}}

{{- /*
The bare service name: the Deployment, Service, ServiceAccount, PDB,
NetworkPolicy and container are all named after it, and it is the selector
label app.kubernetes.io/name. It is NOT derived from the release name: other
services and the GraphQL gateway address the service by its in-namespace
Service short name (http://<serviceName>:<port>). The release name only fills
app.kubernetes.io/instance.
*/ -}}
{{- define "br-common-service.name" -}}
{{- $name := toString (required "serviceName is required: the bare name of the service (Deployment, Service, ServiceAccount and the selector label app.kubernetes.io/name); set it in the service chart" .Values.serviceName) -}}
{{- if and $name (not (regexMatch "^[a-z]([-a-z0-9]{0,61}[a-z0-9])?$" $name)) -}}
{{- fail (printf "serviceName %q is not a DNS-1035 label (lowercase letters, digits and '-', starting with a letter, at most 63 characters)" $name) -}}
{{- end -}}
{{- $name -}}
{{- end -}}

{{- /*
The logical environment (e.g. dev, uat, prod): the ENVIRONMENT variable and
the label botresources.ai/env. No default — the chart never guesses which
environment it is in.
*/ -}}
{{- define "br-common-service.env" -}}
{{- toString (required "env is required: the logical environment this service is deployed in (e.g. dev, uat, prod), set by the deploying repository per environment" .Values.env) -}}
{{- end -}}

{{- define "br-common-service.selectorLabels" -}}
app.kubernetes.io/name: {{ include "br-common-service.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- /*
Labels of every resource AND of the pod template. The five library labels
cannot be replaced through `commonLabels`: two of them are the immutable
Deployment selector.
*/ -}}
{{- define "br-common-service.labels" -}}
{{ include "br-common-service.selectorLabels" . }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/component: backend
botresources.ai/env: {{ include "br-common-service.env" . | quote }}
{{- with include "br-common-service.commonLabels" . }}
{{ . }}
{{- end }}
{{- end -}}

{{- define "br-common-service.commonLabels" -}}
{{- $owned := list "app.kubernetes.io/name" "app.kubernetes.io/instance" "app.kubernetes.io/managed-by" "app.kubernetes.io/component" "botresources.ai/env" -}}
{{- $labels := .Values.commonLabels | default dict -}}
{{- range $key, $value := $labels }}
{{- if has $key $owned }}
{{- fail (printf "commonLabels must not set %s: the library owns that label" $key) }}
{{- end }}
{{- if not (kindIs "string" $value) }}
{{- fail (printf "commonLabels.%s must be a string (quote it): Kubernetes label values are strings" $key) }}
{{- end }}
{{- end }}
{{- with $labels }}{{ toYaml . }}{{ end -}}
{{- end -}}

{{- /* ── Scalars ──────────────────────────────────────────────────────────── */ -}}

{{- /*
A boolean with a default. `default` cannot be used for booleans (it replaces
`false`), and a string such as "false" is refused rather than read as true.
Call with (dict "value" <v> "default" <bool> "field" "<values path>");
returns "true" or "false".
*/ -}}
{{- define "br-common-service.flag" -}}
{{- if kindIs "invalid" .value -}}
{{- .default -}}
{{- else if kindIs "bool" .value -}}
{{- .value -}}
{{- else -}}
{{- fail (printf "%s must be a YAML boolean (true or false), got %v" .field .value) -}}
{{- end -}}
{{- end -}}

{{- /* A TCP port. Call with (dict "value" <v> "field" "<values path>"). */ -}}
{{- define "br-common-service.tcpPort" -}}
{{- $port := int .value -}}
{{- if or (lt $port 1) (gt $port 65535) -}}
{{- fail (printf "%s must be a TCP port (1-65535), got %v" .field .value) -}}
{{- end -}}
{{- $port -}}
{{- end -}}

{{- /*
PORT, the container port and the Service port. No default: each service has
its own port (in BotResources, the one the Projects registry assigns), and a
library default would hide a collision in a shared namespace.
*/ -}}
{{- define "br-common-service.port" -}}
{{- include "br-common-service.tcpPort" (dict "value" (required "port is required: the port the service binary listens on (PORT), the container port and the Service port; set it in the service chart" .Values.port) "field" "port") -}}
{{- end -}}

{{- /*
Replicas. Required: how many pods an environment runs is that environment's
statement. `maxReplicas`, set by the service chart, is the binary's statement:
a service that is not multi-pod safe (an in-process scheduler, an outbox pump
that is not leader-elected) sets 1, and a per-environment value above it fails
the render.
*/ -}}
{{- define "br-common-service.replicas" -}}
{{- $replicas := int (required "replicaCount is required: the number of pods, set by the deploying repository per environment" .Values.replicaCount) -}}
{{- if lt $replicas 0 -}}
{{- fail (printf "replicaCount must be >= 0, got %d" $replicas) -}}
{{- end -}}
{{- if not (kindIs "invalid" .Values.maxReplicas) -}}
{{- if gt $replicas (int .Values.maxReplicas) -}}
{{- fail (printf "replicaCount %d exceeds maxReplicas %d: the service chart states that this binary is not safe with more pods" $replicas (int .Values.maxReplicas)) -}}
{{- end -}}
{{- end -}}
{{- $replicas -}}
{{- end -}}

{{- define "br-common-service.resources" -}}
{{- $resources := .Values.resources | default dict -}}
{{- if not $resources -}}
{{- fail "resources is required: requests and limits of the service container, set by the deploying repository per environment" -}}
{{- end -}}
{{- toYaml $resources -}}
{{- end -}}

{{- /* ── Rollout on Secret change ─────────────────────────────────────────── */ -}}

{{- /*
The Secrets the pod reads, for the Stakater Reloader annotation
`secret.reloader.stakater.com/reload`: the binary reads its DSNs once, at
boot, so a rotated password reaches it only through a new pod. Built from
what the pod actually references, in order and without duplicates: the owner
Secret, the app Secret, every secretKeyRef of `extraEnv` and of
`extraInitContainers`, then every `imagePullSecrets` entry.
*/ -}}
{{- define "br-common-service.reloadSecrets" -}}
{{- $names := list -}}
{{- if eq (include "br-common-service.migrates" .) "true" -}}
{{- $names = append $names (include "br-common-service.ownerSecretName" .) -}}
{{- end -}}
{{- $names = append $names (include "br-common-service.appSecretName" .) -}}
{{- range (.Values.extraEnv | default list) -}}
{{- $names = append $names (dig "valueFrom" "secretKeyRef" "name" "" .) -}}
{{- end -}}
{{- range (.Values.extraInitContainers | default list) -}}
{{- range (get . "env" | default list) -}}
{{- $names = append $names (dig "valueFrom" "secretKeyRef" "name" "" .) -}}
{{- end -}}
{{- end -}}
{{- range (.Values.imagePullSecrets | default list) -}}
{{- $names = append $names (get . "name" | default "") -}}
{{- end -}}
{{- $names | compact | uniq | join "," -}}
{{- end -}}

{{- /* ── Pod hardening ────────────────────────────────────────────────────── */ -}}

{{- /*
The hardened defaults. The service binary runs as any non-root UID, needs no
capability and writes nothing to its root filesystem. A key set in
`podSecurityContext` / `containerSecurityContext` replaces the default key of
the same name, a key set to null removes it, the other defaults stay.
`mergeOverwrite` is not used on purpose: it skips zero values, so
`readOnlyRootFilesystem: false` would silently keep `true`.
*/ -}}
{{- define "br-common-service.defaultPodSecurityContext" -}}
{{- dict "runAsNonRoot" true "runAsUser" 65532 "runAsGroup" 65532 "fsGroup" 65532 "seccompProfile" (dict "type" "RuntimeDefault") | toYaml -}}
{{- end -}}

{{- define "br-common-service.defaultContainerSecurityContext" -}}
{{- dict "allowPrivilegeEscalation" false "readOnlyRootFilesystem" true "capabilities" (dict "drop" (list "ALL")) | toYaml -}}
{{- end -}}

{{- define "br-common-service.overrideByKey" -}}
{{- $out := .defaults | fromYaml -}}
{{- range $key, $value := (.given | default dict) }}
{{- if kindIs "invalid" $value }}
{{- $_ := unset $out $key }}
{{- else }}
{{- $_ := set $out $key $value }}
{{- end }}
{{- end }}
{{- with $out }}{{ toYaml . }}{{ end -}}
{{- end -}}

{{- define "br-common-service.podSecurityContext" -}}
{{- include "br-common-service.overrideByKey" (dict "defaults" (include "br-common-service.defaultPodSecurityContext" .) "given" .Values.podSecurityContext) -}}
{{- end -}}

{{- define "br-common-service.containerSecurityContext" -}}
{{- include "br-common-service.overrideByKey" (dict "defaults" (include "br-common-service.defaultContainerSecurityContext" .) "given" .Values.containerSecurityContext) -}}
{{- end -}}

{{- /* ── Probes ───────────────────────────────────────────────────────────── */ -}}

{{- /*
Probe TIMING. Only the five timing fields are accepted — the path and the port
are the ops contract (`probes.livenessPath`, `probes.readinessPath`, the `http`
port), so any other key fails the render. Call with
(dict "given" <map> "defaults" <map> "field" "<values path>" "skip" <list>).
*/ -}}
{{- define "br-common-service.probeTiming" -}}
{{- $fields := list "initialDelaySeconds" "periodSeconds" "timeoutSeconds" "successThreshold" "failureThreshold" -}}
{{- $skip := .skip | default list -}}
{{- $timing := deepCopy (.defaults | default dict) -}}
{{- range $key, $value := (.given | default dict) }}
{{- if has $key $skip }}
{{- else if not (has $key $fields) }}
{{- fail (printf "%s.%s is not a probe timing field (%s); the probe paths are probes.livenessPath and probes.readinessPath" $.field $key (join ", " $fields)) }}
{{- else }}
{{- $_ := set $timing $key $value }}
{{- end }}
{{- end }}
{{- $lines := list }}
{{- range $fields }}
{{- if hasKey $timing . }}
{{- $lines = append $lines (printf "%s: %d" . (int (index $timing .))) }}
{{- end }}
{{- end }}
{{- join "\n" $lines -}}
{{- end -}}

{{- define "br-common-service.probePath" -}}
{{- $path := toString (default .default .value) -}}
{{- if not (hasPrefix "/" $path) -}}
{{- fail (printf "%s must be an absolute HTTP path, got %q" .field $path) -}}
{{- end -}}
{{- $path -}}
{{- end -}}
