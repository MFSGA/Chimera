
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/dashboard')({
  component: Dashboard,
})

function Dashboard() {
  // todo: use i18n
  // const { t } = useTranslation()

  return (
    <div>dashboard</div>
  )
}
