<template>
  <div class="h-full bg-dark-900 flex flex-col">
    <!-- Background gradient effect -->
    <div class="absolute inset-0 bg-gradient-to-br from-brand-cyan/5 via-transparent to-brand-blue/5 pointer-events-none"></div>

    <div class="relative flex-1 flex flex-col max-w-4xl mx-auto p-5 w-full">
      <!-- Top Section: Two-column layout -->
      <div class="grid grid-cols-2 gap-4 mb-4 flex-1 min-h-0">
        <!-- Left Column: Connection & Network Info -->
        <div class="flex flex-col gap-4 min-h-0">
          <!-- Connection Info (1/3 height) -->
          <div class="bg-dark-800 border border-dark-600 rounded-xl p-5 flex-[1] flex flex-col justify-center">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div class="w-8 h-8 bg-gradient-to-br from-brand-cyan to-brand-blue rounded-lg flex items-center justify-center flex-shrink-0">
                  <svg class="w-5 h-5 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 20l-5.447-2.724A1 1 0 013 16.382V5.618a1 1 0 011.447-.894L9 7m0 13l6-3m-6 3V7m6 10l4.553 2.276A1 1 0 0021 18.382V7.618a1 1 0 00-.553-.894L15 4m0 13V4m0 0L9 7" />
                  </svg>
                </div>
                <div class="min-w-0">
                  <h1 class="text-base font-bold text-white">Cartographer Agent</h1>
                  <p class="text-xs text-gray-400 flex items-center gap-1.5 truncate">
                    <span class="w-2 h-2 rounded-full flex-shrink-0" :class="statusDotClass"></span>
                    <span class="truncate">{{ status.userEmail || 'Unknown' }}</span>
                  </p>
                </div>
              </div>
              <div class="flex items-center gap-1 flex-shrink-0">
                <button
                  @click="handleDisconnect"
                  class="text-gray-400 hover:text-red-400 text-xs transition-colors px-2 py-1"
                  title="Disconnect from cloud"
                >
                  Disconnect
                </button>
                <button
                  @click="$router.push('/preferences')"
                  class="text-gray-400 hover:text-white p-1.5 rounded-lg hover:bg-dark-700 transition-colors"
                  title="Settings"
                >
                  <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  </svg>
                </button>
              </div>
            </div>
            <p v-if="status.networkName" class="text-xs text-brand-cyan mt-2 truncate">
              {{ status.networkName }}
            </p>
          </div>

          <!-- Network Info (2/3 height) -->
          <div class="bg-dark-800 border border-dark-600 rounded-xl p-5 flex-[2] flex flex-col justify-center">
            <h2 class="text-sm font-semibold text-white mb-3 flex items-center gap-2">
              <svg class="w-4 h-4 text-brand-cyan" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 3v2m6-2v2M9 19v2m6-2v2M5 9H3m2 6H3m18-6h-2m2 6h-2M7 19h10a2 2 0 002-2V7a2 2 0 00-2-2H7a2 2 0 00-2 2v10a2 2 0 002 2zM9 9h6v6H9V9z" />
              </svg>
              Network Information
            </h2>
            <div class="space-y-2 text-sm">
              <div>
                <span class="text-gray-400">Network:</span>
                <span class="ml-2 font-mono text-white">{{ networkInfo || 'Detecting...' }}</span>
              </div>
              <div>
                <span class="text-gray-400">Last scan:</span>
                <span class="ml-2 text-white">{{ lastScanTime }}</span>
              </div>
              <div>
                <span class="text-gray-400">Last health check:</span>
                <span class="ml-2 text-white">{{ lastHealthCheckTime }}</span>
              </div>
            </div>
          </div>
        </div>

        <!-- Right Column: Device Health Pie Chart -->
        <div class="bg-dark-800 border border-dark-600 rounded-xl p-5 flex items-center justify-center">
          <DeviceHealthPieChart
            :healthy-count="deviceHealthStats.healthy"
            :degraded-count="deviceHealthStats.degraded"
            :offline-count="deviceHealthStats.offline"
            :size="180"
            :stroke-width="20"
            @view-devices="showDeviceList = true"
          />
        </div>
      </div>

      <!-- Scan Section -->
      <div class="bg-dark-800 border border-dark-600 rounded-xl p-5 mb-4 flex-shrink-0">
        <!-- Scan Controls -->
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-sm font-semibold text-white flex items-center gap-2">
            <svg class="w-4 h-4 text-brand-cyan" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
            </svg>
            Scan Controls
          </h2>
          <div class="flex gap-2">
            <button
              @click="handleHealthCheck"
              :disabled="checkingHealth || devices.length === 0"
              class="bg-emerald-600 hover:bg-emerald-500 disabled:bg-dark-600 disabled:text-gray-500 text-white font-medium py-2 px-4 rounded-lg transition-colors flex items-center gap-2 text-sm"
              title="Check if devices are reachable"
            >
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              {{ checkingHealth ? 'Checking...' : 'Health Check' }}
            </button>
            <button
              v-if="!scanning"
              @click="handleScan"
              class="bg-brand-cyan hover:bg-brand-cyan/90 text-dark-900 font-medium py-2 px-4 rounded-lg transition-colors flex items-center gap-2 text-sm"
            >
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
              Full Scan
            </button>
            <button
              v-else
              @click="handleCancelScan"
              class="bg-red-600 hover:bg-red-500 text-white font-medium py-2 px-4 rounded-lg transition-colors flex items-center gap-2 text-sm"
            >
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
              </svg>
              Cancel
            </button>
          </div>
        </div>

        <!-- Cloud Command Banner -->
        <div v-if="cloudCommand" class="p-4 bg-purple-500/10 border border-purple-500/30 rounded-lg mb-4">
          <div class="flex items-center gap-2">
            <svg class="w-4 h-4 text-purple-400 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 15a4 4 0 004 4h9a5 5 0 10-.1-9.999 5.002 5.002 0 10-9.78 2.096A4.001 4.001 0 003 15z" />
            </svg>
            <span class="text-sm font-medium text-purple-400">
              {{ cloudCommandLabel }}
            </span>
            <div v-if="cloudCommand.stage === 'executing'" class="w-4 h-4 border-2 border-purple-400 border-t-transparent rounded-full animate-spin"></div>
          </div>
          <p class="text-sm text-gray-400 mt-1">{{ cloudCommand.message }}</p>
        </div>

        <!-- Scan Progress -->
        <div v-if="scanProgress" class="p-4 bg-brand-cyan/10 border border-brand-cyan/30 rounded-lg">
          <div class="flex items-center justify-between mb-2">
            <span class="text-sm font-medium text-brand-cyan">
              {{ getScanStageLabel(scanProgress.stage) }}
            </span>
            <span v-if="scanProgress.percent !== null" class="text-sm text-brand-cyan">
              {{ scanProgress.percent }}%
            </span>
          </div>
          <div class="w-full bg-dark-700 rounded-full h-2 mb-2">
            <div
              class="bg-brand-cyan h-2 rounded-full transition-all duration-300"
              :style="{ width: `${scanProgress.percent || 0}%` }"
            ></div>
          </div>
          <p class="text-sm text-gray-400">{{ scanProgress.message }}</p>
          <div class="flex justify-between text-xs text-gray-500 mt-2">
            <span v-if="scanProgress.devicesFound !== null">
              {{ scanProgress.devicesFound }} device{{ scanProgress.devicesFound !== 1 ? 's' : '' }} found
            </span>
            <span>{{ scanElapsedTime.toFixed(1) }}s elapsed</span>
          </div>
        </div>

        <!-- Scan Starting Placeholder -->
        <div v-else-if="scanning" class="p-4 bg-brand-cyan/10 border border-brand-cyan/30 rounded-lg">
          <div class="flex items-center gap-2 mb-2">
            <div class="w-4 h-4 border-2 border-brand-cyan border-t-transparent rounded-full animate-spin"></div>
            <span class="text-sm font-medium text-brand-cyan">Starting Network Scan</span>
          </div>
          <div class="w-full bg-dark-700 rounded-full h-2 mb-2">
            <div class="bg-brand-cyan/50 h-2 rounded-full w-1/4 animate-pulse"></div>
          </div>
          <div class="flex justify-between text-xs text-gray-400">
            <span>Initializing scan, please wait...</span>
            <span>{{ scanElapsedTime.toFixed(1) }}s elapsed</span>
          </div>
        </div>

        <!-- Health Check Progress -->
        <div v-else-if="healthCheckProgress" class="p-4 bg-emerald-500/10 border border-emerald-500/30 rounded-lg">
          <div class="flex items-center justify-between mb-2">
            <span class="text-sm font-medium text-emerald-400">
              {{ getHealthCheckStageLabel(healthCheckProgress.stage) }}
            </span>
            <span v-if="healthCheckProgress.totalDevices > 0" class="text-sm text-emerald-400">
              {{ healthCheckProgress.checkedDevices }}/{{ healthCheckProgress.totalDevices }}
            </span>
          </div>
          <div class="w-full bg-dark-700 rounded-full h-2 mb-2">
            <div
              class="bg-emerald-500 h-2 rounded-full transition-all duration-300"
              :style="{ width: healthCheckProgress.totalDevices > 0 ? `${(healthCheckProgress.checkedDevices / healthCheckProgress.totalDevices) * 100}%` : '0%' }"
            ></div>
          </div>
          <p class="text-sm text-gray-400">{{ healthCheckProgress.message }}</p>
          <div v-if="healthCheckProgress.healthyDevices > 0 || healthCheckProgress.checkedDevices > 0" class="flex justify-between text-xs text-gray-500 mt-2">
            <span class="text-emerald-400">{{ healthCheckProgress.healthyDevices }} healthy</span>
            <span v-if="healthCheckProgress.checkedDevices > healthCheckProgress.healthyDevices" class="text-red-400">
              {{ healthCheckProgress.checkedDevices - healthCheckProgress.healthyDevices }} unreachable
            </span>
          </div>
        </div>

        <!-- Last Scan/Health Check Results (persistent) -->
        <div v-else-if="lastOperationResult" class="p-4 bg-dark-700 rounded-lg">
          <div class="flex items-center justify-between mb-2">
            <span class="text-gray-400 text-sm flex items-center gap-2">
              <svg v-if="lastOperationResult.type === 'health'" class="w-4 h-4 text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              <svg v-else class="w-4 h-4 text-brand-cyan" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
              {{ lastOperationResult.type === 'health' ? 'Last Health Check' : 'Last Full Scan' }}
            </span>
            <span class="text-gray-500 text-sm">{{ formatHealthCheckTime(lastOperationResult.timestamp) }}</span>
          </div>
          <div class="flex items-center justify-between">
            <div class="flex gap-4 text-sm">
              <span class="text-green-400 flex items-center gap-1.5">
                <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                </svg>
                {{ lastOperationResult.healthyDevices }} online
              </span>
              <span v-if="lastOperationResult.unreachableDevices > 0" class="text-red-400 flex items-center gap-1.5">
                <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                </svg>
                {{ lastOperationResult.unreachableDevices }} offline
              </span>
            </div>
            <span v-if="lastOperationResult.syncedToCloud" class="text-brand-cyan text-sm flex items-center gap-1.5">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 15a4 4 0 004 4h9a5 5 0 10-.1-9.999 5.002 5.002 0 10-9.78 2.096A4.001 4.001 0 003 15z" />
              </svg>
              Synced
            </span>
            <span v-else class="text-amber-400 text-sm flex items-center gap-1.5" title="Could not connect to cloud. Data saved locally.">
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 15a4 4 0 004 4h9a5 5 0 10-.1-9.999 5.002 5.002 0 10-9.78 2.096A4.001 4.001 0 003 15z" />
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18.364 5.636l-12.728 12.728" />
              </svg>
              Sync failed
            </span>
          </div>
        </div>

        <!-- No scan results yet -->
        <div v-else class="p-4 bg-dark-700 rounded-lg text-center">
          <p class="text-sm text-gray-400">No recent scan or health check results</p>
        </div>
      </div>

      <!-- View in Cloud Button -->
      <button
        @click="openCloud"
        class="w-full bg-dark-800 hover:bg-dark-700 border border-dark-600 text-white font-medium py-3 px-6 rounded-lg transition-colors flex items-center justify-center gap-2 flex-shrink-0"
      >
        View in Cloud
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
        </svg>
      </button>
    </div>

    <!-- Device List Modal -->
    <div v-if="showDeviceList" class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4" @click.self="showDeviceList = false">
      <div class="bg-dark-800 border border-dark-600 rounded-xl p-6 max-w-lg w-full max-h-[80vh] overflow-hidden flex flex-col">
        <div class="flex items-center justify-between mb-4">
          <div>
            <h2 class="text-lg font-semibold text-white flex items-center gap-2">
              <svg class="w-5 h-5 text-brand-cyan" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
              </svg>
              {{ devices.length }} Device{{ devices.length !== 1 ? 's' : '' }}
            </h2>
            <p v-if="vendorSummary" class="text-xs text-gray-400 mt-1 ml-7">
              {{ vendorSummary }}
            </p>
          </div>
          <button
            @click="showDeviceList = false"
            class="text-gray-400 hover:text-white p-1 rounded-lg hover:bg-dark-700 transition-colors"
          >
            <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
        <div class="overflow-y-auto flex-1">
          <DeviceList :devices="devices" />
        </div>
      </div>
    </div>

    <!-- Disconnect Confirmation Dialog -->
    <ConfirmDialog
      v-model="showDisconnectDialog"
      title="Disconnect"
      message="Are you sure you want to disconnect from the cloud? You can reconnect at any time."
      confirm-text="Disconnect"
      :destructive="true"
      @confirm="performDisconnect"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useAgentStore, type ScanStage, type HealthCheckStage, type HealthCheckProgress, type CloudCommandEvent } from '@/stores/agent'
import DeviceList from '@/components/DeviceList.vue'
import DeviceHealthPieChart from '@/components/DeviceHealthPieChart.vue'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

interface HealthCheckStatus {
  totalDevices: number
  healthyDevices: number
  unreachableDevices: number
  syncedToCloud: boolean
  timestamp: string  // ISO timestamp of when the check was performed
  devices: Array<{
    ip: string
    mac: string | null
    hostname: string | null
    responseTimeMs: number | null
    vendor: string | null
    deviceType: string | null
  }>
}

interface LastOperationResult {
  type: 'health' | 'scan'
  totalDevices: number
  healthyDevices: number
  unreachableDevices: number
  syncedToCloud: boolean
  timestamp: string
}

const agentStore = useAgentStore()
const networkInfo = ref<string>('')
const checkingHealth = ref(false)
const healthStatus = ref<HealthCheckStatus | null>(null)
const showDeviceList = ref(false)
const showDisconnectDialog = ref(false)
const lastOperationResult = ref<LastOperationResult | null>(null)

// Running elapsed time for scans
const scanStartTime = ref<number | null>(null)
const scanElapsedTime = ref<number>(0)
let scanElapsedInterval: ReturnType<typeof setInterval> | null = null

// Use storeToRefs for proper reactivity with Pinia
const { status, devices: storeDevices, scanning, scanProgress, healthCheckProgress, cloudCommand } = storeToRefs(agentStore)

// Create a computed that explicitly tracks device changes
const devices = computed(() => storeDevices.value)

// Start/stop elapsed time timer when scanning state changes
function updateElapsedTime() {
  if (scanStartTime.value) {
    scanElapsedTime.value = (Date.now() - scanStartTime.value) / 1000
  }
}

function startScanTimer() {
  scanStartTime.value = Date.now()
  scanElapsedTime.value = 0
  if (scanElapsedInterval) {
    clearInterval(scanElapsedInterval)
  }
  // Execute immediately so we don't wait for the first interval
  updateElapsedTime()
  scanElapsedInterval = setInterval(updateElapsedTime, 100) // Update every 100ms for smooth display
}

function stopScanTimer() {
  if (scanElapsedInterval) {
    clearInterval(scanElapsedInterval)
    scanElapsedInterval = null
  }
  scanStartTime.value = null
}

// Watch for scanning state changes to start/stop the timer
// Use immediate: true so the timer starts even if scanning is already true on mount (first scan)
watch(scanning, (isScanning) => {
  if (isScanning) {
    startScanTimer()
  } else {
    stopScanTimer()
  }
}, { immediate: true })

// Compute device health statistics from the current devices list
const deviceHealthStats = computed(() => {
  const devs = devices.value
  let healthy = 0
  let degraded = 0
  let offline = 0

  for (const device of devs) {
    // Device has response time data
    if (device.responseTimeMs !== null && device.responseTimeMs !== undefined) {
      if (device.responseTimeMs > 0) {
        // Good ping response
        if (device.responseTimeMs <= 100) {
          healthy++
        } else {
          // High latency = degraded
          degraded++
        }
      } else {
        // responseTimeMs === 0 means ARP detected but no ping response
        // Still consider it healthy (reachable via ARP)
        healthy++
      }
    } else {
      // No response time data = unknown/offline
      offline++
    }
  }

  return { healthy, degraded, offline }
})

// Compute vendor summary for display in the device list modal
const vendorSummary = computed(() => {
  const vendors = devices.value
    .filter(d => d.vendor)
    .map(d => d.vendor!)
  
  if (vendors.length === 0) return null
  
  // Count unique vendors (simplified names)
  const counts: Record<string, number> = {}
  for (const vendor of vendors) {
    // Simplify vendor name (e.g., "Apple, Inc." -> "Apple")
    const simple = vendor.split(/[,\s]/)[0]
    counts[simple] = (counts[simple] || 0) + 1
  }
  
  // Get top 3 vendors
  const top = Object.entries(counts)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 3)
    .map(([name, count]) => `${count} ${name}`)
  
  return top.join(', ')
})

// Determine overall network health status for the indicator dot
type NetworkHealthStatus = 'online' | 'degraded' | 'offline'

const networkHealthStatus = computed<NetworkHealthStatus>(() => {
  // If currently scanning or checking health, maintain previous state (default to online)
  if (scanning.value || checkingHealth.value) {
    return 'online'
  }

  // Use device health stats
  const stats = deviceHealthStats.value
  const total = stats.healthy + stats.degraded + stats.offline

  // All devices offline = offline status
  if (total > 0 && stats.offline === total) {
    return 'offline'
  }

  // Some devices degraded or offline = degraded
  if (stats.degraded > 0 || stats.offline > 0) {
    return 'degraded'
  }

  // All devices healthy = online
  return 'online'
})

const statusDotClass = computed(() => {
  switch (networkHealthStatus.value) {
    case 'online':
      return 'bg-green-500'
    case 'degraded':
      return 'bg-yellow-500'
    case 'offline':
      return 'bg-red-500'
    default:
      return 'bg-green-500'
  }
})

// Get human-readable label for cloud command stage
const cloudCommandLabel = computed(() => {
  switch (cloudCommand.value?.stage) {
    case 'received': return 'Remote Scan Requested'
    case 'executing': return 'Remote Scan In Progress'
    case 'completed': return 'Remote Scan Complete'
    case 'failed': return 'Remote Scan Failed'
    default: return 'Remote Command'
  }
})

// Get human-readable label for scan stage
function getScanStageLabel(stage: ScanStage): string {
  const labels: Record<ScanStage, string> = {
    starting: 'Starting Scan',
    detecting_network: 'Detecting Network',
    reading_arp: 'Reading Known Devices',
    ping_sweep: 'Discovering Devices',
    resolving_hostnames: 'Resolving Hostnames',
    complete: 'Scan Complete',
    failed: 'Scan Failed'
  }
  return labels[stage] || stage
}

// Get human-readable label for health check stage
function getHealthCheckStageLabel(stage: HealthCheckStage): string {
  const labels: Record<HealthCheckStage, string> = {
    starting: 'Starting Health Check',
    checking_devices: 'Checking Devices',
    uploading: 'Syncing Results',
    complete: 'Health Check Complete'
  }
  return labels[stage] || stage
}

// Computed last scan time that updates when status changes
const lastScanTime = computed(() => {
  if (agentStore.status.lastScan) {
    const date = new Date(agentStore.status.lastScan)
    return date.toLocaleString()
  }
  return 'Never'
})

const lastHealthCheckTime = computed(() => {
  if (lastOperationResult.value && lastOperationResult.value.type === 'health') {
    const date = new Date(lastOperationResult.value.timestamp)
    return date.toLocaleString()
  }
  return 'Never'
})

// Format health check timestamp to relative time (e.g., "2 min ago")
function formatHealthCheckTime(timestamp: string): string {
  const date = new Date(timestamp)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffMins = Math.floor(diffMs / 60000)
  const diffHours = Math.floor(diffMs / 3600000)

  if (diffMins < 1) return 'Just now'
  if (diffMins < 60) return `${diffMins} min ago`
  if (diffHours < 24) return `${diffHours} hour${diffHours > 1 ? 's' : ''} ago`
  return date.toLocaleString()
}

async function handleScan() {
  try {
    const scanResult = await agentStore.scanNow()
    // Refresh status and devices to ensure UI is in sync
    await agentStore.refreshStatus()
    await agentStore.loadDevices()
    // Clear health status when scanning new devices
    healthStatus.value = null

    // Update last operation result
    const stats = deviceHealthStats.value
    lastOperationResult.value = {
      type: 'scan',
      totalDevices: devices.value.length,
      healthyDevices: stats.healthy + stats.degraded, // Count degraded as reachable
      unreachableDevices: stats.offline,
      syncedToCloud: scanResult.syncedToCloud,
      timestamp: new Date().toISOString()
    }
  } catch (error) {
    // Don't show error if scan was cancelled
    const errorMsg = error instanceof Error ? error.message : String(error)
    if (!errorMsg.includes('cancelled')) {
      console.error('Scan error:', error)
      alert('Failed to scan network. Please try again.')
    }
  }
}

async function handleCancelScan() {
  await agentStore.cancelScan()
}

async function handleHealthCheck() {
  checkingHealth.value = true
  try {
    const result = await invoke<Omit<HealthCheckStatus, 'timestamp'>>('run_health_check')
    const timestamp = new Date().toISOString()
    // Add timestamp to the result
    healthStatus.value = {
      ...result,
      timestamp
    }
    // Use the devices returned directly from the health check result
    // This ensures we have the updated responseTimeMs values
    // Map null values to undefined to match Device type
    if (result.devices && result.devices.length > 0) {
      const mappedDevices = result.devices.map(d => ({
        ip: d.ip,
        mac: d.mac ?? undefined,
        hostname: d.hostname ?? undefined,
        responseTimeMs: d.responseTimeMs ?? undefined,
        vendor: d.vendor ?? undefined,
        deviceType: d.deviceType ?? undefined
      }))
      agentStore.updateDevices(mappedDevices)
    }
    
    // Update last operation result
    lastOperationResult.value = {
      type: 'health',
      totalDevices: result.totalDevices,
      healthyDevices: result.healthyDevices,
      unreachableDevices: result.unreachableDevices,
      syncedToCloud: result.syncedToCloud,
      timestamp
    }
  } catch (error) {
    console.error('Health check error:', error)
    alert(`Health check failed: ${error}`)
  } finally {
    checkingHealth.value = false
  }
}

async function openCloud() {
  try {
    await invoke('open_cloud_dashboard')
  } catch (error) {
    console.error('Failed to open cloud:', error)
  }
}

function handleDisconnect() {
  showDisconnectDialog.value = true
}

async function performDisconnect() {
  try {
    await agentStore.logout()
    // Navigate back to setup page
    window.location.href = '/'
  } catch (error) {
    console.error('Failed to disconnect:', error)
  }
}

async function loadNetworkInfo() {
  try {
    const info = await invoke<string>('get_network_info')
    networkInfo.value = info
  } catch (error) {
    console.error('Failed to get network info:', error)
  }
}

// Event listener cleanup
let healthCheckUnlisten: UnlistenFn | null = null
let statusRefreshInterval: ReturnType<typeof setInterval> | null = null

onMounted(async () => {
  await agentStore.refreshStatus()
  await agentStore.loadDevices()
  await loadNetworkInfo()

  // Listen for health-check-progress events (from background health checks)
  // to update lastOperationResult when health checks complete
  healthCheckUnlisten = await listen<HealthCheckProgress>('health-check-progress', (event) => {
    const progress = event.payload

    // When a health check completes (from background or manual), update lastOperationResult
    if (progress.stage === 'complete') {
      lastOperationResult.value = {
        type: 'health',
        totalDevices: progress.totalDevices,
        healthyDevices: progress.healthyDevices,
        unreachableDevices: progress.totalDevices - progress.healthyDevices,
        syncedToCloud: progress.syncedToCloud ?? false,
        timestamp: new Date().toISOString()
      }
    }
  })

  // Refresh status periodically (updates lastScan time automatically)
  statusRefreshInterval = setInterval(() => {
    agentStore.refreshStatus()
  }, 30000) // Every 30 seconds
})

onUnmounted(() => {
  // Cleanup event listeners
  if (healthCheckUnlisten) {
    healthCheckUnlisten()
    healthCheckUnlisten = null
  }
  // Cleanup status refresh interval
  if (statusRefreshInterval) {
    clearInterval(statusRefreshInterval)
    statusRefreshInterval = null
  }
  // Cleanup scan elapsed timer
  if (scanElapsedInterval) {
    clearInterval(scanElapsedInterval)
    scanElapsedInterval = null
  }
})
</script>
