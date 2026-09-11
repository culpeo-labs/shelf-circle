// Container registry for the Shelf Circle backend image.
//
// Deployed on its own (before main.bicep) so the image can be built and pushed
// into it before the Container App that references it is created.

targetScope = 'resourceGroup'

@description('Azure region for the registry.')
param location string = resourceGroup().location

@description('Prefix for resource names.')
param namePrefix string = 'shelfcircle'

@description('Environment suffix, e.g. prod / staging.')
param environmentName string = 'prod'

// ACR names are global and alphanumeric-only; the uniqueString keeps it collision-free.
var acrName = toLower('${namePrefix}${environmentName}acr${uniqueString(resourceGroup().id)}')

resource acr 'Microsoft.ContainerRegistry/registries@2023-07-01' = {
  name: acrName
  location: location
  sku: {
    name: 'Basic'
  }
  properties: {
    adminUserEnabled: false
  }
}

output acrName string = acr.name
output acrLoginServer string = acr.properties.loginServer
