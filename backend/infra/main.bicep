// Shelf Circle backend — application infrastructure.
//
// Azure Database for PostgreSQL Flexible Server + Azure Container Apps, with the
// image pulled from an existing ACR (see registry.bicep) via the Container App's
// system-assigned managed identity. Log Analytics backs the Container Apps
// environment. Database migrations run on app startup (`sqlx::migrate!`).
//
// Secrets (pgAdminPassword, googleBooksApiKey) are passed in at deploy time, not
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

@description('Minimum Container App replicas (1 keeps the API warm).')
param minReplicas int = 1

@description('Maximum Container App replicas.')
param maxReplicas int = 3

var lawName = '${namePrefix}-${environmentName}-law'
var envName = '${namePrefix}-${environmentName}-env'
var apiName = '${namePrefix}-${environmentName}-api'
var pgName = toLower('${namePrefix}-${environmentName}-pg-${uniqueString(resourceGroup().id)}')
var dbName = 'shelfcircle'

var acrPullRoleId = subscriptionResourceId(
  'Microsoft.Authorization/roleDefinitions',
  '7f951dda-4ed3-4680-a7ca-43fe172d538d'
)

resource acr 'Microsoft.ContainerRegistry/registries@2023-07-01' existing = {
  name: acrName
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
    type: 'SystemAssigned'
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
          identity: 'system'
        }
      ]
      secrets: concat(
        [
          {
            name: 'database-url'
            value: 'postgresql://${pgAdminLogin}:${pgAdminPassword}@${pg.properties.fullyQualifiedDomainName}:5432/${dbName}?sslmode=require'
          }
        ],
        empty(googleBooksApiKey)
          ? []
          : [
              {
                name: 'google-books-api-key'
                value: googleBooksApiKey
              }
            ]
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
                name: 'RUST_LOG'
                value: 'info'
              }
            ],
            empty(googleBooksApiKey)
              ? []
              : [
                  {
                    name: 'GOOGLE_BOOKS_API_KEY'
                    secretRef: 'google-books-api-key'
                  }
                ]
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
}

// Let the Container App's managed identity pull from ACR. First deploy: the
// initial image pull may retry until this assignment propagates, then the
// revision goes healthy on its own.
resource acrPull 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(acr.id, api.id, acrPullRoleId)
  scope: acr
  properties: {
    roleDefinitionId: acrPullRoleId
    principalId: api.identity.principalId
    principalType: 'ServicePrincipal'
  }
}

output containerAppName string = api.name
output containerAppFqdn string = api.properties.configuration.ingress.fqdn
output postgresFqdn string = pg.properties.fullyQualifiedDomainName
