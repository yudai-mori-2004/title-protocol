FROM node:20-slim

WORKDIR /app

COPY services/irys-uploader/package.json ./
RUN npm install --production

COPY services/irys-uploader/index.js ./

EXPOSE 3001

CMD ["node", "index.js"]
