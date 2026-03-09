/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',
  async rewrites() {
    const mgmtUrl = process.env.BACKEND_MANAGEMENT_URL || 'http://localhost:50055';
    const metricsUrl = process.env.BACKEND_METRICS_URL || 'http://localhost:50054';
    const analysisUrl = process.env.BACKEND_ANALYSIS_URL || 'http://localhost:50053';
    const banditUrl = process.env.BACKEND_BANDIT_URL || 'http://localhost:50056';

    return [
      {
        source: '/api/rpc/management/:path*',
        destination: `${mgmtUrl}/:path*`,
      },
      {
        source: '/api/rpc/metrics/:path*',
        destination: `${metricsUrl}/:path*`,
      },
      {
        source: '/api/rpc/analysis/:path*',
        destination: `${analysisUrl}/:path*`,
      },
      {
        source: '/api/rpc/bandit/:path*',
        destination: `${banditUrl}/:path*`,
      },
    ];
  },
};
module.exports = nextConfig;
