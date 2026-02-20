# Title Protocol インデクサ Dockerfile

FROM node:20-alpine

WORKDIR /app

COPY indexer/package.json indexer/package-lock.json* ./
RUN if [ -f package-lock.json ]; then npm ci; else npm install; fi

COPY indexer/ ./
RUN npm run build && rm -rf src node_modules && \
    if [ -f package-lock.json ]; then npm ci --omit=dev; else npm install --omit=dev; fi

EXPOSE 5000

CMD ["node", "dist/index.js"]
