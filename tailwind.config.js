/** @type {import('tailwindcss').Config} */
module.exports = {
  content: {
    files: ["*.html", "./templates/**/*.html", "./src/**/*.rs"],
  },
  theme: {
    extend: {
      colors: {
        'void-green': '#0F1C18',
        'void-deep': '#08130E',
        'parchment': '#F2F0E9',
        'sage-mist': '#4A635D',
        'sage-light': '#6B8A82',
        'integral-turquoise': '#2A9D8F',
        'integral-turquoise-bright': '#3FB8A4',
        'systemic-yellow': '#E9C46A',
        'systemic-yellow-soft': '#F2D88A',
        'rhizome-ink': '#0A1410',
      },
      fontFamily: {
        'fraunces': ['Fraunces', 'serif'],
        'urbanist': ['Urbanist', 'sans-serif'],
      },
      fontWeight: {
        'soft': '380',
        'breath': '500',
      },
      letterSpacing: {
        'seedling': '0.14em',
        'mycelium': '0.04em',
      },
      borderRadius: {
        'pebble': '28% 28% 28% 28% / 32% 32% 32% 32%',
        'pebble-lg': '32px',
        'pebble-md': '22px',
        'pebble-sm': '14px',
        'pebble-xs': '8px',
        'seed': '50% 50% 50% 50%',
      },
      boxShadow: {
        'glass': '0 2px 16px -2px rgb(0 0 0 / 0.4), inset 0 1px 0 0 rgb(255 255 255 / 0.06)',
        'glass-lg': '0 8px 32px -4px rgb(0 0 0 / 0.5), inset 0 1px 0 0 rgb(255 255 255 / 0.08)',
        'orb': '0 0 24px 0 rgb(42 157 143 / 0.45), 0 8px 24px -4px rgb(0 0 0 / 0.5), inset 0 1px 0 0 rgb(255 255 255 / 0.35)',
        'orb-thinking': '0 0 32px 8px rgb(42 157 143 / 0.35), 0 0 48px 16px rgb(233 196 106 / 0.25)',
        'bubble': '0 1px 2px rgb(0 0 0 / 0.3), inset 0 1px 0 0 rgb(255 255 255 / 0.04)',
      },
      backdropBlur: {
        'membrane': '18px',
        'membrane-lg': '40px',
      },
      animation: {
        'breathe': 'breathe 4s ease-in-out infinite',
        'seed-pulse': 'seed-pulse 2.4s ease-in-out infinite',
        'spin-slow': 'spin 12s linear infinite',
        'mandala': 'mandala 2.6s cubic-bezier(0.5,0,0.5,1) infinite',
        'grow-in': 'grow-in 0.5s cubic-bezier(0.2,0.7,0.2,1) both',
        'fade-in-up': 'fade-in-up 0.4s ease-out both',
        'rise': 'rise 0.6s cubic-bezier(0.2,0.7,0.2,1) both',
        'drift': 'drift 24s ease-in-out infinite alternate',
      },
      keyframes: {
        breathe: {
          '0%, 100%': { transform: 'scale(1)', opacity: '1' },
          '50%': { transform: 'scale(1.06)', opacity: '0.92' },
        },
        'seed-pulse': {
          '0%, 100%': { transform: 'scale(1)', boxShadow: '0 0 24px 0 rgb(42 157 143 / 0.45)' },
          '50%': { transform: 'scale(0.96)', boxShadow: '0 0 36px 6px rgb(233 196 106 / 0.4)' },
        },
        mandala: {
          '0%': { transform: 'rotate(0deg) scale(1)', opacity: '0.6' },
          '50%': { transform: 'rotate(180deg) scale(0.9)', opacity: '1' },
          '100%': { transform: 'rotate(360deg) scale(1)', opacity: '0.6' },
        },
        'grow-in': {
          '0%': { opacity: '0', transform: 'translateY(10px) scale(0.98)' },
          '100%': { opacity: '1', transform: 'translateY(0) scale(1)' },
        },
        'fade-in-up': {
          '0%': { opacity: '0', transform: 'translateY(8px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' },
        },
        rise: {
          '0%': { opacity: '0', transform: 'translateY(24px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' },
        },
        drift: {
          '0%': { transform: 'translate(0,0)' },
          '100%': { transform: 'translate(40px,-30px)' },
        },
      },
    },
  },
  plugins: [],
}
