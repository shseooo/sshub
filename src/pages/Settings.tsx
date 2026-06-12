import { useQueryClient } from '@tanstack/react-query'
import { syncServersToConfig, syncConfigToServers } from '@/lib/tauriCommands'
import { ArrowRight, ArrowLeft } from 'lucide-react'

export default function Settings() {
  const queryClient = useQueryClient()

  const handleSyncToConfig = async () => {
    try {
      await syncServersToConfig()
      alert('SSH config에 동기화했습니다.')
    } catch (error) {
      alert('동기화 실패: ' + error)
    }
  }

  const handleSyncFromConfig = async () => {
    try {
      const servers = await syncConfigToServers()
      alert(`${servers.length}개의 서버를 가져왔습니다.`)
      queryClient.invalidateQueries({ queryKey: ['servers'] })
    } catch (error) {
      alert('가져오기 실패: ' + error)
    }
  }

  return (
    <div className="p-6 max-w-2xl">
      <h1 className="text-2xl font-bold mb-6">설정</h1>

      <div className="space-y-6">
        <div className="bg-card border border-border rounded-lg p-6">
          <h2 className="text-lg font-semibold mb-4">SSH Config 동기화</h2>
          <p className="text-sm text-muted-foreground mb-4">
            ~/.ssh/config 파일과 앱의 서버 목록을 동기화할 수 있습니다.
          </p>

          <div className="space-y-3">
            <div className="flex items-center justify-between p-3 rounded-md bg-muted/50">
              <div>
                <h3 className="font-medium">서버 → SSH Config</h3>
                <p className="text-xs text-muted-foreground">
                  앱의 서버 목록을 ~/.ssh/config에 작성합니다.
                </p>
              </div>
              <button
                onClick={handleSyncToConfig}
                className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 text-sm"
              >
                <ArrowRight className="h-3 w-3" />
                동기화
              </button>
            </div>

            <div className="flex items-center justify-between p-3 rounded-md bg-muted/50">
              <div>
                <h3 className="font-medium">SSH Config → 서버</h3>
                <p className="text-xs text-muted-foreground">
                  ~/.ssh/config에서 서버 정보를 가져옵니다.
                </p>
              </div>
              <button
                onClick={handleSyncFromConfig}
                className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-secondary text-secondary-foreground hover:bg-secondary/80 text-sm"
              >
                <ArrowLeft className="h-3 w-3" />
                가져오기
              </button>
            </div>
          </div>
        </div>

        <div className="bg-card border border-border rounded-lg p-6">
          <h2 className="text-lg font-semibold mb-4">정보</h2>
          <div className="space-y-2 text-sm text-muted-foreground">
            <p>버전: 0.1.0</p>
            <p>데이터 위치: ~/Library/Application Support/com.connectunnel.sshub/</p>
          </div>
        </div>
      </div>
    </div>
  )
}