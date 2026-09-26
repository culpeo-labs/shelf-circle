// Shelf Circle backend — application infrastructure.
//
// Azure Database for PostgreSQL Flexible Server + Azure Container Apps, with the
// image pulled from an existing ACR (see registry.bicep) via the Container App's
// system-assigned managed identity. Log Analytics backs the Container Apps
// environment. Database migrations run on app startup (`sqlx::migrate!`).
//
// Secrets (pgAdminPassword, googleBooksApiKey, hankoApiKey) are passed in at deploy time, not
// stored in source. See infra/main.parameters.json for the non-secret defaults.

targetScope = 'resourceGroup'

@description('Azure region for all resources.')
param location string = resourceGroup().location

@description('Prefix for resource names.')
param namePrefix string = 'shelfcircle'

@description('Environment suffix, e.g. prod / staging.')
param environmentName string = 'prod'

@description('Name of the existing Azure Container Registry (output of registry.bicep).')
param acrName string

@description('Fully-qualified container image reference, e.g. myacr.azurecr.io/shelf-circle-backend:<sha>.')
param containerImage string

@description('PostgreSQL administrator login.')
param pgAdminLogin string = 'shelfcircle'

@description('PostgreSQL administrator password.')
@secure()
param pgAdminPassword string

@description('PostgreSQL Flexible Server compute SKU.')
param pgSkuName string = 'Standard_B1ms'

@description('PostgreSQL Flexible Server compute tier.')
@allowed([
  'Burstable'
  'GeneralPurpose'
  'MemoryOptimized'
])
param pgSkuTier string = 'Burstable'

@description('PostgreSQL storage size in GB.')
param pgStorageGb int = 32

@description('PostgreSQL major version.')
param pgVersion string = '16'

@description('Hanko Cloud project API URL, e.g. https://<project>.hanko.io.')
param hankoApiUrl string

@description('Optional Hanko JWT audience to enforce. Empty disables the aud check.')
param hankoAudience string = ''

@description('Optional Google Books API key. Empty disables the Google Books provider.')
@secure()
param googleBooksApiKey string = ''

@description('Hanko Cloud admin API key, used only to delete a user at Hanko when they delete their account. Empty disables account deletion (DELETE /me answers 503).')
@secure()
param hankoApiKey string = ''

@description('Minimum Container App replicas (1 keeps the API warm).')
param minReplicas int = 1

@description('Maximum Container App replicas.')
param maxReplicas int = 3

var lawName = '${namePrefix}-${environmentName}-law'
var envName = '${namePrefix}-${environmentName}-env'
var apiName = '${namePrefix}-${environmentName}-api'
var pgName = toLower('${namePrefix}-${environmentName}-pg-${uniqueString(resourceGroup().id)}')
var dbName = 'shelfcircle'
// Storage account names: 3-24 chars, lowercase letters/digits only.
var storageName = take(toLower('${namePrefix}${environmentName}st${uniqueString(resourceGroup().id)}'), 24)
var avatarContainerName = 'avatars'

var acrPullRoleId = subscriptionResourceId(
  'Microsoft.Authorization/roleDefinitions',
  '7f951dda-4ed3-4680-a7ca-43fe172d538d'
)

resource acr 'Microsoft.ContainerRegistry/registries@2023-07-01' existing = {
  name: acrName
}

// User-assigned (not system-assigned) so AcrPull can be granted *before* the
// Container App exists: a system-assigned identity's principalId only exists
// once the app resource is created, which makes the role assignment depend on
// the app — but the app's first image pull depends on the role assignment
// already being in place. That circular wait causes the initial revision to
// retry pulling until Container Apps gives up ("Operation expired"), failing
// the whole deployment before the role assignment is ever attempted. A
// pre-existing identity breaks the cycle.
resource apiIdentity 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' = {
  name: '${apiName}-identity'
  location: location
}

resource law 'Microsoft.OperationalInsights/workspaces@2023-09-01' = {
  name: lawName
  location: location
  properties: {
    sku: {
      name: 'PerGB2018'
    }
    retentionInDays: 30
  }
}

resource pg 'Microsoft.DBforPostgreSQL/flexibleServers@2024-08-01' = {
  name: pgName
  location: location
  sku: {
    name: pgSkuName
    tier: pgSkuTier
  }
  properties: {
    version: pgVersion
    administratorLogin: pgAdminLogin
    administratorLoginPassword: pgAdminPassword
    storage: {
      storageSizeGB: pgStorageGb
    }
    backup: {
      backupRetentionDays: 7
      geoRedundantBackup: 'Disabled'
    }
    highAvailability: {
      mode: 'Disabled'
    }
    authConfig: {
      passwordAuth: 'Enabled'
      activeDirectoryAuth: 'Disabled'
    }
    createMode: 'Default'
  }
}

// Container Apps egresses from within Azure; this rule (start=end=0.0.0.0) is the
// "allow all Azure services" special case. Tighten to VNet integration later.
resource pgFirewallAzure 'Microsoft.DBforPostgreSQL/flexibleServers/firewallRules@2024-08-01' = {
  parent: pg
  name: 'AllowAllAzureServicesAndResourcesWithinAzureIps'
  properties: {
    startIpAddress: '0.0.0.0'
    endIpAddress: '0.0.0.0'
  }
}

resource pgDatabase 'Microsoft.DBforPostgreSQL/flexibleServers/databases@2024-08-01' = {
  parent: pg
  name: dbName
  properties: {
    charset: 'UTF8'
    collation: 'en_US.utf8'
  }
}

// Profile pictures. Blobs are named `<user-uuid>/<random-uuid>.jpg` and the
// container allows anonymous *blob-level* read (no listing), so the app can
// show them with a plain image URL. Uploads never go through anonymous access:
// the API hands the app a short-lived write-only SAS signed with the account
// key (see backend/src/storage.rs).
resource storage 'Microsoft.Storage/storageAccounts@2023-05-01' = {
  name: storageName
  location: location
  kind: 'StorageV2'
  sku: {
    name: 'Standard_LRS'
  }
  properties: {
    allowBlobPublicAccess: true
    minimumTlsVersion: 'TLS1_2'
    supportsHttpsTrafficOnly: true
  }
}

resource blobService 'Microsoft.Storage/storageAccounts/blobServices@2023-05-01' = {
  parent: storage
  name: 'default'
}

resource avatarContainer 'Microsoft.Storage/storageAccounts/blobServices/containers@2023-05-01' = {
  parent: blobService
  name: avatarContainerName
  properties: {
    publicAccess: 'Blob'
  }
}

resource env 'Microsoft.App/managedEnvironments@2024-03-01' = {
  name: envName
  location: location
  properties: {
    appLogsConfiguration: {
      destination: 'log-analytics'
      logAnalyticsConfiguration: {
        customerId: law.properties.customerId
        sharedKey: law.listKeys().primarySharedKey
      }
    }
  }
}

resource api 'Microsoft.App/containerApps@2024-03-01' = {
  name: apiName
  location: location
  identity: {
    type: 'UserAssigned'
    userAssignedIdentities: {
      '${apiIdentity.id}': {}
    }
  }
  properties: {
    managedEnvironmentId: env.id
    configuration: {
      activeRevisionsMode: 'Single'
      ingress: {
        external: true
        targetPort: 8080
        transport: 'auto'
        allowInsecure: false
        traffic: [
          {
            latestRevision: true
            weight: 100
          }
        ]
      }
      registries: [
        {
          server: acr.properties.loginServer
          identity: apiIdentity.id
        }
      ]
      secrets: concat(
        [
          {
            name: 'database-url'
            value: 'postgresql://${pgAdminLogin}:${pgAdminPassword}@${pg.properties.fullyQualifiedDomainName}:5432/${dbName}?sslmode=require'
          }
          {
            name: 'azure-storage-key'
            value: storage.listKeys().keys[0].value
          }
        ],
        concat(
          empty(googleBooksApiKey)
            ? []
            : [
                {
                  name: 'google-books-api-key'
                  value: googleBooksApiKey
                }
              ],
          empty(hankoApiKey)
            ? []
            : [
                {
                  name: 'hanko-api-key'
                  value: hankoApiKey
                }
              ]
        )
      )
    }
    template: {
      containers: [
        {
          name: 'api'
          image: containerImage
          resources: {
            cpu: json('0.5')
            memory: '1Gi'
          }
          env: concat(
            [
              {
                name: 'DATABASE_URL'
                secretRef: 'database-url'
              }
              {
                name: 'HANKO_API_URL'
                value: hankoApiUrl
              }
              {
                name: 'HANKO_AUDIENCE'
                value: hankoAudience
              }
              {
                name: 'AZURE_STORAGE_ACCOUNT'
                value: storage.name
              }
              {
                name: 'AZURE_STORAGE_KEY'
                secretRef: 'azure-storage-key'
              }
              {
                name: 'AZURE_STORAGE_CONTAINER'
                value: avatarContainerName
              }
              {
                name: 'RUST_LOG'
                value: 'info'
              }
            ],
            concat(
              empty(googleBooksApiKey)
                ? []
                : [
                    {
                      name: 'GOOGLE_BOOKS_API_KEY'
                      secretRef: 'google-books-api-key'
                    }
                  ],
              empty(hankoApiKey)
                ? []
                : [
                    {
                      name: 'HANKO_API_KEY'
                      secretRef: 'hanko-api-key'
                    }
                  ]
            )
          )
          probes: [
            {
              type: 'Liveness'
              httpGet: {
                path: '/health'
                port: 8080
              }
              initialDelaySeconds: 10
              periodSeconds: 30
            }
            {
              type: 'Readiness'
              httpGet: {
                path: '/health'
                port: 8080
              }
              initialDelaySeconds: 5
              periodSeconds: 10
              failureThreshold: 6
            }
          ]
        }
      ]
      scale: {
        minReplicas: minReplicas
        maxReplicas: maxReplicas
        rules: [
          {
            name: 'http-concurrency'
            http: {
              metadata: {
                concurrentRequests: '50'
              }
            }
          }
        ]
      }
    }
  }
  dependsOn: [
    acrPull
    avatarContainer
  ]
}

// Grant AcrPull to the identity before the Container App is created (see
// apiIdentity above) so the app's first image pull succeeds immediately
// instead of racing role propagation.
resource acrPull 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(acr.id, apiIdentity.id, acrPullRoleId)
  scope: acr
  properties: {
    roleDefinitionId: acrPullRoleId
    principalId: apiIdentity.properties.principalId
    principalType: 'ServicePrincipal'
  }
}

output containerAppName string = api.name
output containerAppFqdn string = api.properties.configuration.ingress.fqdn
output storageAccountName string = storage.name
output postgresFqdn string = pg.properties.fullyQualifiedDomainName
