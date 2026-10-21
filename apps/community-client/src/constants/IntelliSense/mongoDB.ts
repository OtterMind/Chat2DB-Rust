import { DatabaseTypeCode } from '../common';

export default {
  type: DatabaseTypeCode.MONGODB,
  keywords: ['db', 'use', 'collections'],
  functions: [
    //========== db.  ==========
    'help()', // Show help information
    'auth()', // Perform user authentication
    'getMongo()', // Get the current Mongo client connection
    'getName()', // Get the name of the current database
    'stats()', // Get database statistics
    'version()', // Get version information of MongoDB server
    'changeUserPassword()', // Change user password
    'logout()', // Log out of current user
    'runCommand()', // Run a database command
    'adminCommand()', // Run a command on the admin database
    'currentOp()', // Show currently ongoing operations
    'killOp()', // Terminate an ongoing operation
    'shutdownServer()', // Safely shut down the MongoDB server
    'fsyncLock()', // Lock the database in read-only mode
    'fsyncUnlock()', // Unlock the read-only lock on the database
    'serverBits()', // Get the bitness of the operating system architecture
    'serverStatus()', // Get server status
    'serverBuildInfo()', // Get server build information
    'hostInfo()', // Get host information

    'createView()', // Create a new view

    'getUser()', // Get information about a user
    'getUsers()', // Get information about all users
    'createUser()', // Create a new user
    'updateUser()', // Update user information
    'dropUser()', // Delete a user
    'dropAllUsers()', // Delete all users

    'getSiblingDB()', // Get a reference to a sibling database
    'getCollection()', // Get a reference to a collection
    'createCollection()', // Create a new collection
    'getCollectionNames()', // Get the names of all collections
    'getCollectionInfos()', // Get information about a collection

    'getRole()', // Get information about a role
    'getRoles()', // Get information about all roles
    'createRole()', // Create a new role
    'dropRole()', // Delete a role
    'dropAllRoles()', // Delete all roles
    'grantRolesToUser()', // Grant roles to users
    'revokeRolesFromUser()', // Revoke role from user
    'grantRolesToRole()', // Grant roles to roles
    'revokeRolesFromRole()', // Revoke a role from a role
    'grantPrivilegesToRole()', // Grant permissions to roles
    'revokePrivilegesFromRole()', // Revoke permissions from role

    'dropDatabase()', // Delete current database
    // 'cloneDatabase()', // Clone the current database (obsolete)
    // 'copyDatabase()', // Copy the database (obsolete)

    //========== db. End ==========

    // ========== collection. ==========

    // Index management
    'getIndexSpecs()', // Get the specifications of existing indexes on a collection
    'getIndices()', // Get a list of existing indexes on a collection
    'createIndex()', // Create an index on the collection
    // 'ensureIndex()', // Ensure that the index on the collection exists (deprecated)
    'createIndexes()', // Create one or more indexes on a collection
    'dropIndex()', // Delete an index on a collection
    'dropIndexes()', // Drop multiple indexes on a collection
    'totalIndexSize()', // Reports the total size of the collection index
    'getIndexKeys()', // Returns an array of key patterns for the indexes defined on the collection
    'getIndexes()', // Get a list of existing indexes on a collection
    'hideIndex()', // Hide index in collection
    'unhideIndex()', // Unhide an index in a collection
    'reIndex()', // Rebuild all indexes on the collection

    // Query and write operations
    'aggregate()', // Perform aggregation operations on a collection
    'mapReduce()', // Perform MapReduce operations on collections
    'find()', // Query documents in a collection
    'findOne()', // Query a document in a collection
    'insertOne()', // Insert a document into the collection
    'insert()', // Insert one or more documents into a collection
    'insertMany()', // Insert multiple documents into a collection
    'update()', // Update one or more documents in a collection
    'updateOne()', // Update a document in the collection
    'updateMany()', // Updates all documents in the collection that match the specified filter criteria
    'deleteOne()', // Delete a document from the collection
    'deleteMany()', // Delete all documents in the collection that match the filter criteria
    'remove()', // Remove documents from collection
    'findOneAndDelete()', // Find and delete a document
    'findOneAndReplace()', // Find and replace a document
    'findAndModify()', // Find and modify a document
    'findOneAndUpdate()', // Find and update a document
    'replaceOne()', // Replace a document in a collection
    'bulkWrite()', // Perform multiple write operations



    // Collection management
    'validate()', // Validation set
    'isCapped()', // Check if the collection is a capped collection
    'getShardVersion()', // Get the shard version information of a collection
    'getDB()', // Get the current database
    'getFullName()', // Returns the collection name prefixed with the database name
    'getName()', // Return collection name
    'drop()', // Delete collection
    'renameCollection()', // Rename collection
    'convertToCapped()', // Convert a collection to a capped collection
    'distinct()', // Returns a list of unique values in the collection
    'findAnd()', // Find documents in a collection

    // Data and status information
    'stats()', // Return statistics for a collection
    'getMongo()', // Return Mongo object
    'estimatedDocumentCount()', // Returns the estimated number of documents in the collection
    'countDocuments()', // Returns the number of documents matching the query
    'count()', // Returns the number of documents matching the find() query
    'storageSize()', // Returns the total allocated space for collection document storage
    'totalSize()', // Returns the collection data plus the total size of all indexes
    'dataSize()', // Returns the size of the collection data
    'latencyStats()', // Returns latency statistics for a collection

    // Query planning and optimization
    'getPlanCache()', // Get the query plan cache interface of the collection
    'explain()', // Return query plan information

    // Sharding and replication
    'getShardDistribution()', // Print data distribution statistics of sharded collections
    'watch()', // Open a change stream on the collection

    // Others
    'runCommand()', // Run database command with given name
    'exists()', // Check if the collection exists

    // ========== collection. End ==========


    // conditional command
    '$eq', // equal to
    '$gt', // greater than
    '$gte', // Greater than or equal to
    '$lt', // less than
    '$lte', // less than or equal to
    '$ne', // not equal to
    '$in', // in list
    '$nin', // not in list
    '$and', // logical AND
    '$not', // logical negation
    '$nor', // Logical NOR
    '$or', // logical or
    '$exists', // Determine whether the field exists
    '$type', // Determine field type
    '$mod', // Modulo operation
    '$regex', // regular expression
    '$text', // Text search
    '$where', // JavaScript expression
    '$all', // Contains all
    '$elemMatch', // Contains matching elements
    '$bitsAllSet', // Bit operations
    '$bitsAnySet', // Bit operations
    '$bitsAllClear', // Bit operations
    '$bitsAnyClear', // Bit operations
    '$comment', // Comment
    '$geoIntersects', // geospatial query
    '$geoWithin', // geospatial query
    '$near', // geospatial query
    '$nearSphere', // geospatial query
    '$allElementsTrue', // Array operations
    '$elemMatch', // Array operations
    '$lookup', // Aggregation operation
    '$match', // Aggregation operation
    '$project', // Aggregation operation
    '$redact', // Aggregation operation
    '$unwind', // Aggregation operation
    '$add', // arithmetic operations
    '$subtract', // arithmetic operations
    '$multiply', // arithmetic operations
    '$divide', // arithmetic operations
    '$mod', // arithmetic operations
    '$concat', // String operations
    '$strcasecmp', // String operations
    '$substr', // String operations
    '$toLower', // String operations
    '$toUpper', // String operations
    '$arrayElemAt', // Array operations
    '$arrayToObject', // Array operations
    '$concatArrays', // Array operations
    '$filter', // Array operations
    '$in', // Array operations
    '$indexOfArray', // Array operations
    '$isArray', // Array operations
    '$map', // Array operations
    '$objectToArray', // Array operations
    '$range', // Array operations
    '$reduce', // Array operations
    '$reverseArray', // Array operations
    '$size', // Array operations
    '$slice', // Array operations
    '$zip', // Array operations
    '$dateToString', // Date operations
    '$dateFromString', // Date operations
    '$dateToParts', // Date operations
    '$dateFromParts', // Date operations
    '$isoDayOfWeek', // Date operations
    '$isoWeek', // Date operations
    '$isoWeekYear', // Date operations
    '$dayOfMonth', // Date operations
    '$dayOfWeek', // Date operations
    '$dayOfYear', // Date operations
    '$month', // Date operations
    '$week', // Date operations
    '$year', // Date operations
    '$hour', // Date operations
    '$minute', // Date operations
    '$second', // Date operations
    '$millisecond', // Date operations






  ],
};
