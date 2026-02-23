import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export interface Device {
  ip: string
  mac?: string
  responseTimeMs?: number
  hostname?: string
  /** Device vendor/manufacturer from MAC OUI lookup */
  vendor?: string
  /** Inferred device type based on vendor (e.g., "router", "apple", "nas", "iot") */
  deviceType?: string
}

export interface AgentStatus {
  authenticated: boolean
  userEmail?: string
  networkId?: string
  networkName?: string
  lastScan?: string
  nextScan?: string
  deviceCount?: number
  scanningInProgress?: boolean
}

export type ScanStage =
  | 'starting'
  | 'detecting_network'
  | 'reading_arp'
  | 'ping_sweep'
  | 'resolving_hostnames'
  | 'complete'
  | 'failed'

export interface ScanProgress {
  stage: ScanStage
  message: string
  percent: number | null
  devicesFound: number | null
  elapsedSecs: number
}

export type HealthCheckStage = 'starting' | 'checking_devices' | 'uploading' | 'complete'

export interface HealthCheckProgress {
  stage: HealthCheckStage
  message: string
  totalDevices: number
  checkedDevices: number
  healthyDevices: number
  /** Whether the health check was synced to cloud (only present on 'complete' stage) */
  syncedToCloud?: boolean
}

export type CloudCommandStage = 'received' | 'executing' | 'completed' | 'failed'

export interface CloudCommandEvent {
  stage: CloudCommandStage
  commandId: number
  commandType: string
  message: string
}

/** Response from scan_network command */
export interface ScanResultResponse {
  devices: Device[]
  syncedToCloud: boolean
}

export interface LoginFlowResponse {
  verificationUrl: string
  userCode: string
  deviceCode: string
  expiresIn: number
  pollInterval: number
}

export const useAgentStore = defineStore('agent', () => {
  const status = ref<AgentStatus>({
    authenticated: false
  })
  
  const devices = ref<Device[]>([])
  const scanning = ref(false)
  const scanInterval = ref(5) // minutes
  const scanProgress = ref<ScanProgress | null>(null)
  const healthCheckProgress = ref<HealthCheckProgress | null>(null)
  const cloudCommand = ref<CloudCommandEvent | null>(null)

  // Event listener cleanup
  let progressUnlisten: UnlistenFn | null = null
  let healthUnlisten: UnlistenFn | null = null
  let scanCompleteUnlisten: UnlistenFn | null = null
  let cloudCommandUnlisten: UnlistenFn | null = null

  const isAuthenticated = computed(() => status.value.authenticated)

  // Initialize event listeners
  async function initEventListeners() {
    // Listen for scan progress events
    progressUnlisten = await listen<ScanProgress>('scan-progress', (event) => {
      scanProgress.value = event.payload

      // Set scanning state based on progress - this handles background scans too
      if (event.payload.stage === 'complete' || event.payload.stage === 'failed') {
        scanning.value = false
        setTimeout(() => {
          scanProgress.value = null
        }, 3000) // Keep final message visible for 3 seconds
      } else {
        scanning.value = true
      }
    })

    // Listen for scan-complete events from background scans
    // This ensures devices are reloaded after background scans complete
    scanCompleteUnlisten = await listen<number>('scan-complete', async (event) => {
      console.log(`Background scan complete, found ${event.payload} devices`)
      // Reload devices to get the updated list from the backend
      try {
        const result = await invoke<Device[]>('get_devices')
        devices.value = result
      } catch (error) {
        console.error('Failed to reload devices after scan:', error)
      }
      // Also refresh status to update lastScan time
      try {
        const statusResult = await invoke<AgentStatus>('get_agent_status')
        status.value = statusResult
      } catch (error) {
        console.error('Failed to refresh status after scan:', error)
      }
    })

    // Listen for health check progress events
    healthUnlisten = await listen<HealthCheckProgress>('health-check-progress', (event) => {
      healthCheckProgress.value = event.payload
      // Clear progress when health check completes
      if (event.payload.stage === 'complete') {
        setTimeout(() => {
          healthCheckProgress.value = null
        }, 3000) // Keep final message visible for 3 seconds

        // Reload devices to get updated health data after background health checks
        invoke<Device[]>('get_devices')
          .then(result => {
            devices.value = result
          })
          .catch(error => {
            console.error('Failed to reload devices after health check:', error)
          })
      }
    })

    // Listen for cloud command events (remote scan triggers from cloud UI)
    cloudCommandUnlisten = await listen<CloudCommandEvent>('cloud-command', (event) => {
      cloudCommand.value = event.payload
      // Auto-clear after completion or failure
      if (event.payload.stage === 'completed' || event.payload.stage === 'failed') {
        setTimeout(() => {
          cloudCommand.value = null
        }, 5000)
      }
    })
  }

  // Cleanup event listeners
  function cleanupEventListeners() {
    if (progressUnlisten) {
      progressUnlisten()
      progressUnlisten = null
    }
    if (healthUnlisten) {
      healthUnlisten()
      healthUnlisten = null
    }
    if (scanCompleteUnlisten) {
      scanCompleteUnlisten()
      scanCompleteUnlisten = null
    }
    if (cloudCommandUnlisten) {
      cloudCommandUnlisten()
      cloudCommandUnlisten = null
    }
  }

  async function checkAuth() {
    try {
      const result = await invoke<AgentStatus>('check_auth_status')
      status.value = result
      // Sync scanning state from backend
      if (result.scanningInProgress) {
        scanning.value = true
      }
      return result.authenticated
    } catch (error) {
      console.error('Failed to check auth status:', error)
      return false
    }
  }

  async function login() {
    try {
      const result = await invoke<AgentStatus>('start_login_flow')
      status.value = result
      // Sync scanning state from backend - scan starts immediately after login
      if (result.scanningInProgress) {
        scanning.value = true
      }
      return result.authenticated
    } catch (error) {
      console.error('Login failed:', error)
      throw error
    }
  }

  /**
   * Request the login URL. This returns immediately with the verification URL.
   * Use completeLogin() afterwards to wait for the user to complete authorization.
   */
  async function requestLogin(): Promise<LoginFlowResponse> {
    const result = await invoke<LoginFlowResponse>('request_login')
    return result
  }

  /**
   * Complete the login flow by polling for token completion.
   * Call this after requestLogin() to wait for user authorization.
   */
  async function completeLogin(deviceCode: string, expiresIn: number, pollInterval: number): Promise<boolean> {
    try {
      const result = await invoke<AgentStatus>('complete_login', {
        deviceCode,
        expiresIn,
        pollInterval
      })
      status.value = result
      // Sync scanning state from backend - scan starts immediately after login
      if (result.scanningInProgress) {
        scanning.value = true
      }
      return result.authenticated
    } catch (error) {
      console.error('Login completion failed:', error)
      throw error
    }
  }

  async function logout() {
    try {
      await invoke('logout')
      // Clear all local state so reconnecting starts with a clean slate
      status.value = { authenticated: false }
      devices.value = []
      scanning.value = false
      scanProgress.value = null
      healthCheckProgress.value = null
    } catch (error) {
      console.error('Logout failed:', error)
      throw error
    }
  }

  async function scanNow(): Promise<ScanResultResponse> {
    scanning.value = true
    scanProgress.value = null
    try {
      const result = await invoke<ScanResultResponse>('scan_network')
      // Force update by creating a new array reference
      devices.value = [...result.devices]
      await refreshStatus()
      return result
    } catch (error) {
      console.error('Scan failed:', error)
      throw error
    } finally {
      scanning.value = false
    }
  }

  async function cancelScan() {
    try {
      await invoke('cancel_scan')
      scanning.value = false
      scanProgress.value = null
    } catch (error) {
      console.error('Failed to cancel scan:', error)
    }
  }

  async function refreshStatus() {
    try {
      const result = await invoke<AgentStatus>('get_agent_status')
      status.value = result
      // Sync scanning state from backend - ensures UI shows scan in progress
      // even if we missed the initial scan-progress events
      if (result.scanningInProgress) {
        scanning.value = true
      }
    } catch (error) {
      console.error('Failed to refresh status:', error)
    }
  }

  async function loadDevices() {
    try {
      const result = await invoke<Device[]>('get_devices')
      devices.value = result
    } catch (error) {
      console.error('Failed to load devices:', error)
    }
  }

  async function setScanInterval(minutes: number) {
    scanInterval.value = minutes
    try {
      await invoke('set_scan_interval', { minutes })
    } catch (error) {
      console.error('Failed to set scan interval:', error)
      throw error
    }
  }

  // Update devices with new data (e.g., after health check)
  function updateDevices(newDevices: Device[]) {
    devices.value = newDevices
  }

  // Initialize listeners on store creation
  initEventListeners()

  return {
    status,
    devices,
    scanning,
    scanInterval,
    scanProgress,
    healthCheckProgress,
    cloudCommand,
    isAuthenticated,
    checkAuth,
    login,
    requestLogin,
    completeLogin,
    logout,
    scanNow,
    cancelScan,
    refreshStatus,
    loadDevices,
    setScanInterval,
    updateDevices,
    cleanupEventListeners
  }
})

