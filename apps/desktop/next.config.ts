import type { NextConfig } from "next";

// Next.js runs only as a static site inside Tauri's WebView (SRS 3.2):
// no SSR, API routes, server actions, or middleware.
const nextConfig: NextConfig = {
  output: "export",
  trailingSlash: true,
  images: { unoptimized: true },
  reactStrictMode: true,
  devIndicators: false,
};

export default nextConfig;
