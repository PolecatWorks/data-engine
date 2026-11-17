{{/*
Expand the name of the chart.
*/}}
{{- define "data-engine.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this (by the DNS naming spec).
If release name contains chart name it will be used as a full name.
*/}}
{{- define "data-engine.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "data-engine.config" -}}
{{- $baseconfig := fromYaml (.Files.Get "configs/config.yaml") }}
{{- $newconfig := default dict .Values.config }}
{{- $postmerge := mergeOverwrite $baseconfig $newconfig }}
{{- tpl (toYaml $postmerge) . }}
{{- end -}}

{{- define "data-engine.volumes" -}}
{{- tpl (toYaml .Values.volumes) . }}
{{- end -}}

{{- define "data-engine.volumeMounts" -}}
{{- tpl (toYaml .Values.volumeMounts) . }}
{{- end -}}

{{- define "data-engine.env" -}}
{{- tpl (toYaml .Values.env) . }}
{{- end -}}

{{- define "data-engine.initContainer.env" -}}
{{- tpl (toYaml .Values.initContainer.env) . }}
{{- end -}}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "data-engine.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/*
Selector labels for be
*/}}
{{- define "data-engine.selectorLabels" -}}
app.kubernetes.io/name: be
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Selector labels for fe
*/}}
{{- define "data-engine.fe.selectorLabels" -}}
app.kubernetes.io/name: fe
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Common labels for be
*/}}
{{- define "data-engine.labels" -}}
helm.sh/chart: {{ include "data-engine.chart" . }}
{{ include "data-engine.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- if .Values.labels }}
{{ toYaml .Values.labels }}
{{- end }}
{{- end }}

{{/*
Common labels for fe
*/}}
{{- define "data-engine.fe.labels" -}}
helm.sh/chart: {{ include "data-engine.chart" . }}
{{ include "data-engine.fe.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- if .Values.fe.labels }}
{{ toYaml .Values.fe.labels }}
{{- end }}
{{- end }}

{{/*
Create the name of the service account to use for be
*/}}
{{- define "data-engine.serviceAccountName" -}}
{{- default (printf "%s-be" (include "data-engine.fullname" .)) .Values.serviceAccount.name }}
{{- end }}

{{/*
Create the name of the service account to use for fe
*/}}
{{- define "data-engine.fe.serviceAccountName" -}}
{{- default (printf "%s-fe" (include "data-engine.fullname" .)) .Values.fe.serviceAccount.name }}
{{- end }}
