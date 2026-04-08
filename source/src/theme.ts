const THEME_KEY = 'yeti-theme'

const themes = ['#0e0e0e', '#0a0e14', '#0a120a', '#120e0a']

try {
  const saved = localStorage.getItem(THEME_KEY)
  if (saved !== null) {
    const idx = parseInt(saved, 10)
    if (idx >= 0 && idx < themes.length) {
      document.documentElement.style.setProperty('--color-black', themes[idx])
    }
  }
} catch {}
