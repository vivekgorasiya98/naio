/** PM2 process config — run on nms.taurus-tech.in API server */
export default {
  apps: [
    {
      name: 'niao-nms',
      script: 'src/server.js',
      cwd: import.meta.dirname,
      instances: 1,
      autorestart: true,
      watch: false,
      max_memory_restart: '256M',
      env: {
        NODE_ENV: 'production',
      },
    },
  ],
};
