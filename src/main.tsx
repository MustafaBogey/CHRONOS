import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './styles/chronos.css'
import './styles/remediation.css'

/* Shared inline SVG gradients from the approved prototype (ringGradient, qualityGradient, qualityArea). */
const GradientDefs = () => (
  <svg width="0" height="0" style={{ position: 'absolute' }} aria-hidden="true">
    <defs>
      <linearGradient id="ringGradient">
        <stop stopColor="#e85eff" /><stop offset=".35" stopColor="#b23dff" />
        <stop offset=".75" stopColor="#913cff" /><stop offset="1" stopColor="#713dff" />
      </linearGradient>
      <linearGradient id="qualityGradient" x1="0" y1="0" x2="1" y2="1">
        <stop stopColor="#1ad9ff" /><stop offset=".5" stopColor="#00f2c6" /><stop offset="1" stopColor="#0cf7a9" />
      </linearGradient>
      <linearGradient id="qualityArea" x1="0" y1="0" x2="0" y2="1">
        <stop stopColor="#c44dff" stopOpacity=".26" /><stop offset="1" stopColor="#c44dff" stopOpacity="0" />
      </linearGradient>
    </defs>
  </svg>
)

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <GradientDefs />
    <App />
  </React.StrictMode>,
)
