/// <reference types="vitest" />
import { defineConfig } from 'vitest/config'

import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// https://vitejs.dev/config/
export default defineConfig({
    plugins: [
        react(),
        tailwindcss(),
    ],
    test: {
        environment: 'jsdom',
        globals: true,
        environmentOptions: {
            jsdom: {
                url: 'http://localhost:3000',
            },
        },
    },
    server: {
        proxy: {
            // Forward API calls to Axum
            '/api': {
                target: 'http://127.0.0.1:3000',
                ws: true,
            },

            // Forward media requests to Axum's static file servers
            '/uploads': 'http://127.0.0.1:3000',
            '/thumbnails': 'http://127.0.0.1:3000',
        }
    }
})
