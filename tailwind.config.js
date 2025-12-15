/** @type {import('tailwindcss').Config} */
module.exports = {
  content: {
    files: ["*.html", "./src/**/*.rs"],
  },
  theme: {
    extend: {
      colors: {
        'void-green': '#0F1C18',
        'parchment': '#F2F0E9',
        'sage-mist': '#4A635D',
        'integral-turquoise': '#2A9D8F',
        'systemic-yellow': '#E9C46A',
      },
      fontFamily: {
        'fraunces': ['Fraunces', 'serif'],
        'urbanist': ['Urbanist', 'sans-serif'],
      },
      animation: {
        'breathe': 'breathe 3s ease-in-out infinite',
        'spin-slow': 'spin 8s linear infinite',
      },
      keyframes: {
        breathe: {
          '0%, 100%': { transform: 'scale(1)' },
          '50%': { transform: 'scale(1.05)' },
        }
      }
    },
  },
  plugins: [],
}
