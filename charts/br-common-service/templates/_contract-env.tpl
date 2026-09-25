{{- /*
br-common-service — the environment variables of the ops contract: the
Postgres DSN parts, NATS, and the guard that keeps the contract variables the
library's. Same rules as _helpers.tpl: the TOP-LEVEL `.Values` of the service
chart, no default for a per-environment value.

EVERY NAME IS THE CODE'S. A variable the binary reads is named by the
br-rust-common constant that reads it — ENVIRONMENT, PORT, DATABASE_URL and
NATS_URL by br_util_boot::env, DATABASE_URL_OWNER and TRUSTED_NETWORK_HOSTS by
br_util_postgres::env — and .github/scripts/check-chart.sh fails when a
rendered name differs from its constant (ci/ops-contract.json). The only
names the chart owns are the ones only the chart uses: the DSN parts PGUSER,
PGPASSWORD, PGUSER_OWNER, PGPASSWORD_OWNER, which the kubelet interpolates
into the two DSNs, and POSTGRES_HOST, POSTGRES_PORT of its own
wait-for-postgres container (README.md, "Names").
*/ -}}

{{- /* ── Postgres ─────────────────────────────────────────────────────────── */ -}}

{{- /* "true" when the binary migrates its database at boot as the owner role. */ -}}
{{- define "br-common-service.migrates" -}}
{{- include "br-common-service.flag" (dict "value" (.Values.postgres | default dict).migrate "default" true "field" "postgres.migrate") -}}
{{- end -}}

{{- define "br-common-service.postgresHost" -}}
{{- toString (required "postgres.host is required: the Postgres read-write Service of this environment (e.g. the CNPG <cluster>-rw Service), set by the deploying repository per environment" (.Values.postgres | default dict).host) -}}
{{- end -}}

{{- define "br-common-service.postgresPort" -}}
{{- include "br-common-service.tcpPort" (dict "value" (required "postgres.port is required: the port of postgres.host (5432 for a CNPG <cluster>-rw Service), set by the deploying repository per environment" (.Values.postgres | default dict).port) "field" "postgres.port") -}}
{{- end -}}

{{- define "br-common-service.postgresDatabase" -}}
{{- toString (required "postgres.database is required: the database the service owns; set it in the service chart" (.Values.postgres | default dict).database) -}}
{{- end -}}

{{- define "br-common-service.appSecretName" -}}
{{- toString (required "postgres.appSecretName is required: the Secret (keys username, password) of the least-privilege runtime role behind DATABASE_URL, set by the deploying repository. It must never name the owner Secret: the owner role bypasses row-level security" (.Values.postgres | default dict).appSecretName) -}}
{{- end -}}

{{- define "br-common-service.ownerSecretName" -}}
{{- $pg := .Values.postgres | default dict -}}
{{- $owner := toString (required "postgres.ownerSecretName is required: the Secret (keys username, password) of the database owner role behind DATABASE_URL_OWNER, used only to migrate at boot, set by the deploying repository. Set postgres.migrate=false for a service that never migrates" $pg.ownerSecretName) -}}
{{- if and $owner (eq $owner (toString $pg.appSecretName)) -}}
{{- fail "postgres.ownerSecretName and postgres.appSecretName name the same Secret: the runtime pool would run as the owner role, which bypasses row-level security" -}}
{{- end -}}
{{- $owner -}}
{{- end -}}

{{- /*
TRUSTED_NETWORK_HOSTS (br_util_postgres::env::TRUSTED_NETWORK_HOSTS): the
hosts a DSN may reach WITHOUT TLS. The library builds both DSNs from `postgres.host`, so the only host the
pod ever connects to is that one: `postgres.trustedNetwork: true` trusts
exactly it, and nothing else can be listed; `false` requires TLS, and both
DSNs then end with `?sslmode=<postgres.sslMode>` (br-common-service.dsnQuery),
without which br-util-postgres refuses a remote DSN at boot.

REQUIRED, no default: it is a per-environment statement (an in-namespace
Postgres Service without TLS, such as CNPG's <cluster>-rw, is trusted; any
other host is not), and either default would be wrong somewhere at RUNTIME
while the render succeeds — `false` crash-loops a pod on a plaintext Service,
`true` sends passwords in clear to a host that should have spoken TLS.

`required` lets `false` through (it refuses only nil and ""); under `helm
lint` a missing value reaches the flag check as "", which is not reported a
second time.
*/ -}}
{{- define "br-common-service.trustedNetwork" -}}
{{- $value := required "postgres.trustedNetwork is required: true when postgres.host speaks plaintext on a trusted network (an in-namespace Postgres Service without TLS), false to require TLS; set per environment by the deploying repository" (.Values.postgres | default dict).trustedNetwork -}}
{{- if and (kindIs "string" $value) (eq $value "") -}}
false
{{- else -}}
{{- include "br-common-service.flag" (dict "value" $value "default" false "field" "postgres.trustedNetwork") -}}
{{- end -}}
{{- end -}}

{{- /*
The query both DSNs end with: "" on a trusted network, `?sslmode=<mode>`
otherwise. br-util-postgres reads the TLS mode from the DSN (sqlx falls back
to PGSSLMODE, then to `prefer`, which it refuses on a remote host), so a TLS
Postgres needs the mode IN the URL — without it the render succeeds and every
pod crash-loops at boot.

`postgres.sslMode` is REQUIRED when `trustedNetwork` is false, with no
default: the mode is a statement about that environment's server. `require`
encrypts without checking the server certificate; `verify-ca` / `verify-full`
check it against the trust roots of the binary's TLS stack (br-util-postgres
builds sqlx with the public webpki roots), so they fit a server whose
certificate a public CA signed. On a trusted network the mode is refused: the
host speaks plaintext, and one statement is enough.

A missing `trustedNetwork` renders nothing here: the helper
br-common-service.trustedNetwork reports it, once. Under `helm lint` a
missing mode reaches the enum check as "", which is not reported a second
time.
*/ -}}
{{- define "br-common-service.dsnQuery" -}}
{{- $pg := .Values.postgres | default dict -}}
{{- if or (kindIs "invalid" $pg.trustedNetwork) (eq (toString $pg.trustedNetwork) "") -}}
{{- else if eq (include "br-common-service.trustedNetwork" .) "true" -}}
{{- if not (kindIs "invalid" $pg.sslMode) -}}
{{- fail "postgres.sslMode is set but postgres.trustedNetwork is true: a trusted host speaks plaintext. Set postgres.trustedNetwork: false to require TLS, or remove postgres.sslMode" -}}
{{- end -}}
{{- else -}}
{{- $mode := toString (required "postgres.sslMode is required when postgres.trustedNetwork is false: the TLS mode both DSNs carry (require, verify-ca or verify-full), set per environment by the deploying repository" $pg.sslMode) -}}
{{- if and $mode (not (has $mode (list "require" "verify-ca" "verify-full"))) -}}
{{- fail (printf "postgres.sslMode must be require, verify-ca or verify-full, got %q: br-util-postgres refuses any other mode on a remote host" $mode) -}}
{{- end -}}
{{- with $mode -}}?sslmode={{ . }}{{- end -}}
{{- end -}}
{{- end -}}

{{- /*
THE CONTRACT VARIABLES ARE THE LIBRARY'S. Kubernetes keeps the LAST of two
env entries with the same name, so an `extraEnv` entry named like a contract
variable would silently replace it: `DATABASE_URL: $(DATABASE_URL_OWNER)`
would run every request as the owner role, which bypasses row-level
security; an extra `TRUSTED_NETWORK_HOSTS` would lift TLS for other hosts.
The same holds for `postgres.appPasswordEnv`, and an `extraEnv` entry may
neither repeat it nor repeat another entry. The list below is every variable
the library renders; the chart gate proves each is refused.

Init containers are not checked: each has its own environment (a psql wait
container may legitimately read the owner DSN).
*/ -}}
{{- define "br-common-service.checkEnv" -}}
{{- $owned := list "ENVIRONMENT" "PORT" "TRUSTED_NETWORK_HOSTS" "PGUSER" "PGPASSWORD" "DATABASE_URL" "PGUSER_OWNER" "PGPASSWORD_OWNER" "DATABASE_URL_OWNER" "NATS_URL" -}}
{{- $appPasswordEnv := toString ((.Values.postgres | default dict).appPasswordEnv | default "") -}}
{{- if has $appPasswordEnv $owned -}}
{{- fail (printf "postgres.appPasswordEnv must not be %s: the library sets that variable" $appPasswordEnv) -}}
{{- end -}}
{{- $seen := list -}}
{{- range $i, $entry := (.Values.extraEnv | default list) -}}
{{- $name := "" -}}
{{- if kindIs "map" $entry -}}
{{- $name = toString (get $entry "name" | default "") -}}
{{- end -}}
{{- if not $name -}}
{{- fail (printf "extraEnv[%d] has no name" $i) -}}
{{- else if has $name $owned -}}
{{- fail (printf "extraEnv must not set %s: the library renders it from its own values, and a second entry would silently replace it" $name) -}}
{{- else if eq $name $appPasswordEnv -}}
{{- fail (printf "extraEnv must not set %s: postgres.appPasswordEnv already sets it from postgres.appSecretName" $name) -}}
{{- else if has $name $seen -}}
{{- fail (printf "extraEnv declares %s twice: Kubernetes would keep only the last one" $name) -}}
{{- end -}}
{{- $seen = append $seen $name -}}
{{- end -}}
{{- end -}}

{{- define "br-common-service.envVarName" -}}
{{- if not (regexMatch "^[A-Z_][A-Z0-9_]*$" (toString .value)) -}}
{{- fail (printf "%s %q is not an environment variable name ([A-Z_][A-Z0-9_]*)" .field (toString .value)) -}}
{{- end -}}
{{- .value -}}
{{- end -}}

{{- /* ── NATS ─────────────────────────────────────────────────────────────── */ -}}

{{- /* NATS_URL (br_util_boot::env::NATS_URL). */ -}}
{{- define "br-common-service.natsUrl" -}}
{{- toString (required "nats.url is required: the NATS server of this environment (NATS_URL), set by the deploying repository per environment" (.Values.nats | default dict).url) -}}
{{- end -}}
