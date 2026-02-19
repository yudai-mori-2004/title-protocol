# Title Protocol インデクサ Dockerfile

FROM node:20-alpine

WORKDIR /app

COPY indexer/package.json indexer/package-lock.json* ./
RUN npm ci --production

COPY indexer/ ./
RUN npm run build

EXPOSE 5000

CMD ["node", "dist/index.js"]
