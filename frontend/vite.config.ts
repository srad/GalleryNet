import {defineConfig} from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// https://vitejs.dev/config/
export default defineConfig({
    plugins: [
        react(),
        tailwindcss(),
    ],
    server: {
        proxy: {
            // Forward API calls to Axum
            '/api': 'http://127.0.0.1:3000',

            // Forward media requests to Axum's static file servers
            '/uploads': 'http://127.0.0.1:3000',
            '/thumbnails': 'http://127.0.0.1:3000',
        }
    }
})