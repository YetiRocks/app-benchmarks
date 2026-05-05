import { useAuth } from './hooks/useAuth'
import Login from './pages/Login'
import Benchmarks from './pages/Benchmarks'

export default function App() {
  const authenticated = useAuth()
  if (authenticated === null) return <div className="empty-state">Loading...</div>
  if (!authenticated) return <Login />
  return <Benchmarks />
}
