FROM node:22-alpine AS build

WORKDIR /app

COPY package.json package-lock.json* ./
RUN npm install --no-audit --no-fund

COPY . .
RUN npm run test && npm run build

FROM nginx:alpine

ENV SERVE_ROOT=/mfe/data-engine

COPY --from=build /app/dist/de-fe-container/browser /usr/share/nginx/html
COPY nginx.conf /etc/nginx/templates/default.conf.template

EXPOSE 8080
CMD ["nginx", "-g", "daemon off;"]
