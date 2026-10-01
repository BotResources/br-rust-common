{{- /*
br-rust-common-chart.deployment — the service Deployment.

One container, named after the service, running the image's default command
(or `args`). Boot sequence the binary implements (br-util-boot,
br-util-postgres, br-util-nats-fabric, br-util-scope-declaration): read its
boot environment (br_util_boot::BootEnv), migrate its database through the
owner DSN (DATABASE_URL_OWNER), open the runtime pool through the app DSN
(DATABASE_URL), bind NATS (NATS_URL), serve HTTP on PORT — liveness answers as
soon as the listener is up, readiness only once the service can serve.

Every variable name and both default probe paths are constants of the crates
(README.md, "Names"); .github/scripts/check-chart.sh fails when this template
renders one that differs from its constant.
*/ -}}
{{- define "br-rust-common-chart.deployment" -}}
{{- include "br-rust-common-chart.checkEnv" . -}}
{{- $name := include "br-rust-common-chart.name" . -}}
{{- $port := include "br-rust-common-chart.port" . -}}
{{- $pgHost := include "br-rust-common-chart.postgresHost" . -}}
{{- $pgPort := include "br-rust-common-chart.postgresPort" . -}}
{{- $pgDatabase := include "br-rust-common-chart.postgresDatabase" . -}}
{{- $dsnQuery := include "br-rust-common-chart.dsnQuery" . -}}
{{- $appSecret := include "br-rust-common-chart.appSecretName" . -}}
{{- $migrates := eq (include "br-rust-common-chart.migrates" .) "true" -}}
{{- $pg := .Values.postgres | default dict -}}
{{- $image := .Values.image | default dict -}}
{{- $wait := .Values.waitForPostgres | default dict -}}
{{- $probes := .Values.probes | default dict -}}
{{- $startup := $probes.startup | default dict -}}
{{- /* The defaults are br_util_observability::LIVENESS_PATH and br_util_axum_readiness::READINESS_PATH. */ -}}
{{- $livenessPath := include "br-rust-common-chart.probePath" (dict "value" $probes.livenessPath "default" "/livez" "field" "probes.livenessPath") -}}
{{- $readinessPath := include "br-rust-common-chart.probePath" (dict "value" $probes.readinessPath "default" "/readyz" "field" "probes.readinessPath") -}}
{{- /*
extraVolumes / extraVolumeMounts: pod volumes and mounts of the service
container, rendered verbatim, only when set. A mount must name a declared
volume: the API server would refuse the Deployment at apply time, so the
render refuses it first.
*/ -}}
{{- $volumeNames := list -}}
{{- range $i, $volume := (.Values.extraVolumes | default list) -}}
{{- if not (get $volume "name") -}}
{{- fail (printf "extraVolumes[%d] has no name" $i) -}}
{{- end -}}
{{- if has $volume.name $volumeNames -}}
{{- fail (printf "extraVolumes declares %s twice" $volume.name) -}}
{{- end -}}
{{- $volumeNames = append $volumeNames $volume.name -}}
{{- end -}}
{{- range $i, $mount := (.Values.extraVolumeMounts | default list) -}}
{{- if not (has (get $mount "name" | default "") $volumeNames) -}}
{{- fail (printf "extraVolumeMounts[%d] mounts %q, which extraVolumes does not declare" $i (get $mount "name" | default "")) -}}
{{- end -}}
{{- end -}}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ $name }}
  labels:
    {{- include "br-rust-common-chart.labels" . | nindent 4 }}
  annotations:
    secret.reloader.stakater.com/reload: {{ include "br-rust-common-chart.reloadSecrets" . | quote }}
spec:
  replicas: {{ include "br-rust-common-chart.replicas" . }}
  {{- with .Values.strategy }}
  strategy:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  selector:
    matchLabels:
      {{- include "br-rust-common-chart.selectorLabels" . | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "br-rust-common-chart.labels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ $name }}
      {{- /* No BotResources Rust service calls the Kubernetes API. */}}
      automountServiceAccountToken: {{ include "br-rust-common-chart.flag" (dict "value" .Values.automountServiceAccountToken "default" false "field" "automountServiceAccountToken") }}
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with include "br-rust-common-chart.podSecurityContext" . }}
      securityContext:
        {{- . | nindent 8 }}
      {{- end }}
      {{- $waitEnabled := eq (include "br-rust-common-chart.flag" (dict "value" $wait.enabled "default" true "field" "waitForPostgres.enabled")) "true" }}
      {{- if or $waitEnabled .Values.extraInitContainers }}
      initContainers:
        {{- if $waitEnabled }}
        {{- /*
        Wait for the Postgres Service to accept TCP before the service boots:
        the binary migrates on every start (advisory-locked, safe with several
        pods), and a pod that crash-loops on a not-yet-ready database only
        delays itself with back-off. The default image is busybox 1.36 pinned
        to the digest of its multi-arch index (amd64 and arm64 among others),
        so the tag cannot move under a running environment.
        */}}
        - name: wait-for-postgres
          image: {{ $wait.image | default "busybox:1.36@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662" | quote }}
          command:
            - sh
            - -c
            - |
              echo "Waiting for postgres at ${POSTGRES_HOST}:${POSTGRES_PORT} ..."
              until nc -z "${POSTGRES_HOST}" "${POSTGRES_PORT}"; do sleep 2; done
              echo "Postgres ready."
          env:
            - name: POSTGRES_HOST
              value: {{ $pgHost | quote }}
            - name: POSTGRES_PORT
              value: {{ $pgPort | quote }}
          {{- with include "br-rust-common-chart.containerSecurityContext" . }}
          securityContext:
            {{- . | nindent 12 }}
          {{- end }}
        {{- end }}
        {{- /* An extra init container without a securityContext gets the hardened container defaults. */}}
        {{- $extra := list }}
        {{- range (.Values.extraInitContainers | default list) }}
        {{- $container := deepCopy . }}
        {{- if not (hasKey $container "securityContext") }}
        {{- $_ := set $container "securityContext" (include "br-rust-common-chart.containerSecurityContext" $ | fromYaml) }}
        {{- end }}
        {{- $extra = append $extra $container }}
        {{- end }}
        {{- with $extra }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      {{- end }}
      containers:
        - name: {{ $name }}
          image: {{ include "br-rust-common-chart.image" . | quote }}
          imagePullPolicy: {{ $image.pullPolicy | default "IfNotPresent" }}
          {{- with .Values.args }}
          args:
            {{- toYaml . | nindent 12 }}
          {{- end }}
          ports:
            - name: http
              containerPort: {{ $port }}
              protocol: TCP
          env:
            - name: ENVIRONMENT
              value: {{ include "br-rust-common-chart.env" . | quote }}
            - name: PORT
              value: {{ $port | quote }}
            {{- if eq (include "br-rust-common-chart.trustedNetwork" .) "true" }}
            {{- /*
            br-util-postgres refuses a remote DSN without an sslmode of
            require, verify-ca or verify-full unless its host is listed here.
            Both DSNs below point at postgres.host, so exactly that host is
            trusted, and nothing else. Off a trusted network, both DSNs carry
            the sslmode instead ($dsnQuery).
            */}}
            - name: TRUSTED_NETWORK_HOSTS
              value: {{ $pgHost | quote }}
            {{- end }}
            {{- /*
            TWO-ROLE POSTGRES. The runtime pool (DATABASE_URL) runs as the
            least-privilege app role; the migration pool (DATABASE_URL_OWNER,
            read by br_util_postgres::init_migration_pool at boot, then closed)
            runs as the owner role, which bypasses row-level security and never
            backs a request. Kubernetes expands $(VAR) only against entries
            declared EARLIER in this list, so each role's credentials come
            before the URL that interpolates them.

            PASSWORDS MUST BE URL-SAFE — ALPHANUMERIC ONLY. Both DSNs are built
            by raw string interpolation: a password containing a URL-reserved
            character (/ + = @ : # ? %) corrupts the URL, and the driver fails
            with "password authentication failed" although the Secret and the
            role agree. Generate every role password from an alphanumeric
            charset (tr -dc 'A-Za-z0-9' </dev/urandom | head -c 32), never
            base64 (openssl rand -base64).
            */}}
            - name: PGUSER
              valueFrom:
                secretKeyRef:
                  name: {{ $appSecret }}
                  key: username
            - name: PGPASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ $appSecret }}
                  key: password
            {{- with $pg.appPasswordEnv }}
            {{- /* The app role's password under the name the binary reads to provision that role at boot. */}}
            - name: {{ include "br-rust-common-chart.envVarName" (dict "value" . "field" "postgres.appPasswordEnv") }}
              valueFrom:
                secretKeyRef:
                  name: {{ $appSecret }}
                  key: password
            {{- end }}
            - name: DATABASE_URL
              value: "postgres://$(PGUSER):$(PGPASSWORD)@{{ $pgHost }}:{{ $pgPort }}/{{ $pgDatabase }}{{ $dsnQuery }}"
            {{- if $migrates }}
            {{- $ownerSecret := include "br-rust-common-chart.ownerSecretName" . }}
            - name: PGUSER_OWNER
              valueFrom:
                secretKeyRef:
                  name: {{ $ownerSecret }}
                  key: username
            - name: PGPASSWORD_OWNER
              valueFrom:
                secretKeyRef:
                  name: {{ $ownerSecret }}
                  key: password
            - name: DATABASE_URL_OWNER
              value: "postgres://$(PGUSER_OWNER):$(PGPASSWORD_OWNER)@{{ $pgHost }}:{{ $pgPort }}/{{ $pgDatabase }}{{ $dsnQuery }}"
            {{- end }}
            - name: NATS_URL
              value: {{ include "br-rust-common-chart.natsUrl" . | quote }}
            {{- with .Values.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- /*
          Readiness gates traffic and can stay DOWN for long on a healthy pod
          (e.g. until the scope-declaration handshake completes), so it never
          backs liveness: a slow handshake must not restart the pod.
          */}}
          readinessProbe:
            httpGet:
              path: {{ $readinessPath }}
              port: http
            {{- include "br-rust-common-chart.probeTiming" (dict "given" $probes.readiness "defaults" (dict "initialDelaySeconds" 5 "periodSeconds" 5) "field" "probes.readiness") | nindent 12 }}
          livenessProbe:
            httpGet:
              path: {{ $livenessPath }}
              port: http
            {{- include "br-rust-common-chart.probeTiming" (dict "given" $probes.liveness "defaults" (dict "initialDelaySeconds" 30 "periodSeconds" 10) "field" "probes.liveness") | nindent 12 }}
          {{- if eq (include "br-rust-common-chart.flag" (dict "value" $startup.enabled "default" false "field" "probes.startup.enabled")) "true" }}
          {{- /* Liveness answers once the listener is bound, i.e. at the end of boot: the startup budget covers a slow migration. */}}
          startupProbe:
            httpGet:
              path: {{ $livenessPath }}
              port: http
            {{- include "br-rust-common-chart.probeTiming" (dict "given" $startup "defaults" (dict "periodSeconds" 5 "failureThreshold" 30) "field" "probes.startup" "skip" (list "enabled")) | nindent 12 }}
          {{- end }}
          resources:
            {{- include "br-rust-common-chart.resources" . | nindent 12 }}
          {{- with include "br-rust-common-chart.containerSecurityContext" . }}
          securityContext:
            {{- . | nindent 12 }}
          {{- end }}
          {{- /* E.g. an emptyDir at /tmp for a runtime that writes scratch files, with readOnlyRootFilesystem kept. */}}
          {{- with .Values.extraVolumeMounts }}
          volumeMounts:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- if eq (include "br-rust-common-chart.flag" (dict "value" .Values.topologySpreadEnabled "default" false "field" "topologySpreadEnabled")) "true" }}
      topologySpreadConstraints:
        - maxSkew: 1
          topologyKey: kubernetes.io/hostname
          whenUnsatisfiable: ScheduleAnyway
          labelSelector:
            matchLabels:
              {{- include "br-rust-common-chart.selectorLabels" . | nindent 14 }}
      {{- end }}
      {{- with .Values.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.extraVolumes }}
      volumes:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end -}}
