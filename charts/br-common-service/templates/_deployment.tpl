{{- /*
br-common-service.deployment — the service Deployment.

One container, named after the service, running the image's default command
(or `args`). Boot sequence the binary implements (br-util-postgres,
br-util-nats-fabric, br-util-scope-declaration): migrate its database through
the owner DSN (DATABASE_URL_OWNER), open the runtime pool through the app DSN
(DATABASE_URL), bind NATS (NATS_URL), serve HTTP on PORT — liveness answers as
soon as the listener is up, readiness only once the service can serve.
*/ -}}
{{- define "br-common-service.deployment" -}}
{{- include "br-common-service.checkEnv" . -}}
{{- $name := include "br-common-service.name" . -}}
{{- $port := include "br-common-service.port" . -}}
{{- $pgHost := include "br-common-service.postgresHost" . -}}
{{- $pgPort := include "br-common-service.postgresPort" . -}}
{{- $pgDatabase := include "br-common-service.postgresDatabase" . -}}
{{- $dsnQuery := include "br-common-service.dsnQuery" . -}}
{{- $appSecret := include "br-common-service.appSecretName" . -}}
{{- $migrates := eq (include "br-common-service.migrates" .) "true" -}}
{{- $pg := .Values.postgres | default dict -}}
{{- $image := .Values.image | default dict -}}
{{- $wait := .Values.waitForPostgres | default dict -}}
{{- $probes := .Values.probes | default dict -}}
{{- $startup := $probes.startup | default dict -}}
{{- $livenessPath := include "br-common-service.probePath" (dict "value" $probes.livenessPath "default" "/livez" "field" "probes.livenessPath") -}}
{{- $readinessPath := include "br-common-service.probePath" (dict "value" $probes.readinessPath "default" "/readyz" "field" "probes.readinessPath") -}}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ $name }}
  labels:
    {{- include "br-common-service.labels" . | nindent 4 }}
  annotations:
    secret.reloader.stakater.com/reload: {{ include "br-common-service.reloadSecrets" . | quote }}
spec:
  replicas: {{ include "br-common-service.replicas" . }}
  {{- with .Values.strategy }}
  strategy:
    {{- toYaml . | nindent 4 }}
  {{- end }}
  selector:
    matchLabels:
      {{- include "br-common-service.selectorLabels" . | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "br-common-service.labels" . | nindent 8 }}
    spec:
      serviceAccountName: {{ $name }}
      {{- /* No BotResources Rust service calls the Kubernetes API. */}}
      automountServiceAccountToken: {{ include "br-common-service.flag" (dict "value" .Values.automountServiceAccountToken "default" false "field" "automountServiceAccountToken") }}
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with include "br-common-service.podSecurityContext" . }}
      securityContext:
        {{- . | nindent 8 }}
      {{- end }}
      {{- $waitEnabled := eq (include "br-common-service.flag" (dict "value" $wait.enabled "default" true "field" "waitForPostgres.enabled")) "true" }}
      {{- if or $waitEnabled .Values.extraInitContainers }}
      initContainers:
        {{- if $waitEnabled }}
        {{- /*
        Wait for the Postgres Service to accept TCP before the service boots:
        the binary migrates on every start (advisory-locked, safe with several
        pods), and a pod that crash-loops on a not-yet-ready database only
        delays itself with back-off.
        */}}
        - name: wait-for-postgres
          image: {{ $wait.image | default "busybox:1.36" | quote }}
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
          {{- with include "br-common-service.containerSecurityContext" . }}
          securityContext:
            {{- . | nindent 12 }}
          {{- end }}
        {{- end }}
        {{- /* An extra init container without a securityContext gets the hardened container defaults. */}}
        {{- $extra := list }}
        {{- range (.Values.extraInitContainers | default list) }}
        {{- $container := deepCopy . }}
        {{- if not (hasKey $container "securityContext") }}
        {{- $_ := set $container "securityContext" (include "br-common-service.containerSecurityContext" $ | fromYaml) }}
        {{- end }}
        {{- $extra = append $extra $container }}
        {{- end }}
        {{- with $extra }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      {{- end }}
      containers:
        - name: {{ $name }}
          image: {{ include "br-common-service.image" . | quote }}
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
              value: {{ include "br-common-service.env" . | quote }}
            - name: PORT
              value: {{ $port | quote }}
            {{- if eq (include "br-common-service.trustedNetwork" .) "true" }}
            {{- /*
            br-util-postgres refuses a remote DSN without an sslmode of
            require, verify-ca or verify-full unless its host is listed here.
            Both DSNs below point at postgres.host, so exactly that host is
            trusted — never ALLOW_INSECURE_DATABASE, which would trust every
            host. Off a trusted network, both DSNs carry the sslmode instead
            ($dsnQuery).
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
            - name: {{ include "br-common-service.envVarName" (dict "value" . "field" "postgres.appPasswordEnv") }}
              valueFrom:
                secretKeyRef:
                  name: {{ $appSecret }}
                  key: password
            {{- end }}
            - name: DATABASE_URL
              value: "postgres://$(PGUSER):$(PGPASSWORD)@{{ $pgHost }}:{{ $pgPort }}/{{ $pgDatabase }}{{ $dsnQuery }}"
            {{- if $migrates }}
            {{- $ownerSecret := include "br-common-service.ownerSecretName" . }}
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
              value: {{ include "br-common-service.natsUrl" . | quote }}
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
            {{- include "br-common-service.probeTiming" (dict "given" $probes.readiness "defaults" (dict "initialDelaySeconds" 5 "periodSeconds" 5) "field" "probes.readiness") | nindent 12 }}
          livenessProbe:
            httpGet:
              path: {{ $livenessPath }}
              port: http
            {{- include "br-common-service.probeTiming" (dict "given" $probes.liveness "defaults" (dict "initialDelaySeconds" 30 "periodSeconds" 10) "field" "probes.liveness") | nindent 12 }}
          {{- if eq (include "br-common-service.flag" (dict "value" $startup.enabled "default" false "field" "probes.startup.enabled")) "true" }}
          {{- /* Liveness answers once the listener is bound, i.e. at the end of boot: the startup budget covers a slow migration. */}}
          startupProbe:
            httpGet:
              path: {{ $livenessPath }}
              port: http
            {{- include "br-common-service.probeTiming" (dict "given" $startup "defaults" (dict "periodSeconds" 5 "failureThreshold" 30) "field" "probes.startup" "skip" (list "enabled")) | nindent 12 }}
          {{- end }}
          resources:
            {{- include "br-common-service.resources" . | nindent 12 }}
          {{- with include "br-common-service.containerSecurityContext" . }}
          securityContext:
            {{- . | nindent 12 }}
          {{- end }}
      {{- if eq (include "br-common-service.flag" (dict "value" .Values.topologySpreadEnabled "default" false "field" "topologySpreadEnabled")) "true" }}
      topologySpreadConstraints:
        - maxSkew: 1
          topologyKey: kubernetes.io/hostname
          whenUnsatisfiable: ScheduleAnyway
          labelSelector:
            matchLabels:
              {{- include "br-common-service.selectorLabels" . | nindent 14 }}
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
{{- end -}}
